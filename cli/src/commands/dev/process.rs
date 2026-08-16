use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::log::LogSink;
use crate::ui;

pub fn spawn_labeled(
    mut cmd: Command,
    dir: &std::path::Path,
    label: &'static str,
    sink: Arc<LogSink>,
) -> Child {
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
    spawn_printer(stdout, label, ui::Stream::Out, sink.clone());
    spawn_printer(stderr, label, ui::Stream::Err, sink);
    child
}

pub fn spawn_printer<R>(reader: R, label: &'static str, stream: ui::Stream, sink: Arc<LogSink>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            sink.write_line(stream, label, &line);
        }
    });
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
        name: &'static str,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
        mut spawn: F,
    ) -> Self
    where
        F: FnMut() -> Child + Send + 'static,
    {
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
                            name,
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

    #[test]
    fn backoff_doubles_and_caps() {
        let s1 = next_backoff(MIN_BACKOFF);
        assert_eq!(s1, std::time::Duration::from_secs(2));
        assert_eq!(next_backoff(s1), std::time::Duration::from_secs(4));
        assert_eq!(next_backoff(std::time::Duration::from_secs(8)), MAX_BACKOFF);
    }
}
