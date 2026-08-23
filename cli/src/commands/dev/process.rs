use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::log::LogSink;
use crate::ui;

pub fn spawn_labeled(
    mut cmd: Command,
    dir: &std::path::Path,
    label: impl Into<String>,
    sink: Arc<LogSink>,
) -> Child {
    let label = label.into();
    ui::apply_child_env(&mut cmd);

    #[cfg(unix)]
    cmd.process_group(0);

    // This runs inside the `FnMut() -> Child` closure the supervisor owns, so
    // there is no `Result` to return here — a failure to spawn is terminal.
    let mut child = cmd
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            ui::fatal(&format!("failed to spawn the {label} process: {e}"));
            std::process::exit(1);
        });

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());
    spawn_printer(stdout, label.clone(), ui::Stream::Out, sink.clone());
    spawn_printer(stderr, label, ui::Stream::Err, sink);
    child
}

/// How long a partial line may sit unterminated before it is emitted anyway.
/// A prompt ("Ok to proceed? (y) ") never carries a newline, so a plain line
/// reader would hold it until the child dies — which reads as a silent hang.
const PARTIAL_FLUSH: std::time::Duration = std::time::Duration::from_millis(750);

pub fn spawn_printer<R>(reader: R, label: String, stream: ui::Stream, sink: Arc<LogSink>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    tokio::spawn(async move {
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match tokio::time::timeout(PARTIAL_FLUSH, reader.read(&mut buf)).await {
                // Idle with something buffered: the child is most likely waiting
                // on stdin. Emit what it wrote and start a fresh line.
                Err(_) => flush_pending(&mut pending, stream, &label, &sink),
                Ok(Ok(0)) => {
                    flush_pending(&mut pending, stream, &label, &sink);
                    break;
                }
                Ok(Ok(n)) => {
                    pending.extend_from_slice(&buf[..n]);
                    while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = pending.drain(..=end).collect();
                        emit_bytes(&line[..end], stream, &label, &sink);
                    }
                }
                Ok(Err(_)) => break,
            }
        }
    });
}

fn flush_pending(pending: &mut Vec<u8>, stream: ui::Stream, label: &str, sink: &Arc<LogSink>) {
    if pending.is_empty() {
        return;
    }
    emit_bytes(pending, stream, label, sink);
    pending.clear();
}

fn emit_bytes(bytes: &[u8], stream: ui::Stream, label: &str, sink: &Arc<LogSink>) {
    let text = String::from_utf8_lossy(bytes);
    sink.write_line(stream, label, text.trim_end_matches('\r'));
}

const GRACEFUL_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
const MIN_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(8);
const RESET_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

pub fn next_backoff(current: std::time::Duration) -> std::time::Duration {
    current.saturating_mul(2).min(MAX_BACKOFF)
}

/// SIGTERM the process group, wait briefly, then SIGKILL leftovers.
pub async fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    let pid = child.id();
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
        }
    }

    match tokio::time::timeout(GRACEFUL_WAIT, child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            #[cfg(unix)]
            if let Some(pid) = pid {
                unsafe {
                    libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
        }
    }
}

#[derive(Clone)]
pub struct Supervisor {
    slot: Arc<Mutex<Option<Child>>>,
    restart_requested: Arc<AtomicBool>,
}

impl Supervisor {
    pub fn start<F>(
        name: impl Into<String>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
        mut spawn: F,
    ) -> Self
    where
        F: FnMut() -> Child + Send + 'static,
    {
        let name = name.into();
        let slot = Arc::new(Mutex::new(None));
        let restart_requested = Arc::new(AtomicBool::new(false));
        let slot_task = slot.clone();
        let restart_flag = restart_requested.clone();
        tokio::spawn(async move {
            let mut backoff = MIN_BACKOFF;
            loop {
                if *shutdown.borrow() {
                    break;
                }

                let child = spawn();
                {
                    *slot_task.lock().await = Some(child);
                }

                let started = std::time::Instant::now();
                tokio::select! {
                    _ = wait_slot(&slot_task) => {
                        if *shutdown.borrow() {
                            break;
                        }
                        if restart_flag.swap(false, Ordering::SeqCst) {
                            backoff = MIN_BACKOFF;
                            continue;
                        }
                        if started.elapsed() >= RESET_AFTER {
                            backoff = MIN_BACKOFF;
                        }
                        ui::emit(ui::Stream::Err, "");
                        ui::prefixed(
                            ui::Stream::Err,
                            &name,
                            &format!("process exited — restarting in {}s", backoff.as_secs()),
                        );
                        tokio::select! {
                            _ = tokio::time::sleep(backoff) => {}
                            _ = wait_shutdown(&mut shutdown) => break,
                        }
                        backoff = next_backoff(backoff);
                    }
                    _ = wait_shutdown(&mut shutdown) => {
                        if let Some(child) = slot_task.lock().await.as_mut() {
                            terminate_child(child).await;
                        }
                        break;
                    }
                }
            }
            *slot_task.lock().await = None;
        });
        Self {
            slot,
            restart_requested,
        }
    }

    pub async fn restart(&self) {
        self.restart_requested.store(true, Ordering::SeqCst);
        if let Some(child) = self.slot.lock().await.as_mut() {
            terminate_child(child).await;
        }
    }

    pub async fn shutdown(&self) {
        if let Some(child) = self.slot.lock().await.as_mut() {
            terminate_child(child).await;
        }
    }
}

async fn wait_slot(slot: &Arc<Mutex<Option<Child>>>) {
    // Release the lock while waiting by taking the wait future after a short
    // lock to read try_wait in a loop — Child::wait needs &mut, so we poll.
    loop {
        {
            let mut guard = slot.lock().await;
            if let Some(child) = guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => {}
                    Err(_) => return,
                }
            } else {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn wait_shutdown(rx: &mut tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow_and_update() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prompt arrives without a trailing newline; it must still be forwarded.
    #[tokio::test(flavor = "current_thread")]
    async fn an_unterminated_prompt_is_flushed_when_the_child_goes_quiet() {
        let root = std::env::temp_dir().join("erno-dev-printer-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let sink = Arc::new(LogSink::new(&root));

        let (mut writer, reader) = tokio::io::duplex(64);
        spawn_printer(reader, "app".into(), ui::Stream::Out, sink);
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"done\nOk to proceed? (y) ")
            .await
            .unwrap();

        tokio::time::sleep(PARTIAL_FLUSH + std::time::Duration::from_millis(250)).await;

        let log = std::fs::read_to_string(root.join(".erno/dev.log")).unwrap();
        assert!(log.contains("[app] done\n"), "{log}");
        assert!(log.contains("[app] Ok to proceed? (y) \n"), "{log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let s1 = next_backoff(MIN_BACKOFF);
        assert_eq!(s1, std::time::Duration::from_secs(2));
        assert_eq!(next_backoff(s1), std::time::Duration::from_secs(4));
        assert_eq!(next_backoff(std::time::Duration::from_secs(8)), MAX_BACKOFF);
    }
}
