use std::path::Path;
use std::process::Command;

pub fn argv(file: &str, line: u32) -> Vec<String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    argv_with(&editor, file, line)
}

pub fn argv_with(editor: &str, file: &str, line: u32) -> Vec<String> {
    let bin = Path::new(editor)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(editor);
    match bin {
        "code" | "code-insiders" | "codium" | "cursor" => {
            vec![editor.to_string(), "-g".into(), format!("{file}:{line}")]
        }
        _ => vec![editor.to_string(), format!("+{line}"), file.to_string()],
    }
}

pub fn open(file: &str, line: u32) -> Result<(), String> {
    let args = argv(file, line);
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vi_uses_plus_line() {
        assert_eq!(
            argv_with("vi", "src/a.rs", 12),
            vec!["vi", "+12", "src/a.rs"]
        );
    }

    #[test]
    fn code_uses_goto() {
        assert_eq!(
            argv_with("code", "src/a.rs", 12),
            vec!["code", "-g", "src/a.rs:12"]
        );
    }
}
