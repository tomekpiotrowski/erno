//! The CLI writes to the terminal in exactly one place.
//!
//! Before `src/ui.rs` existed, ~250 call sites each made their own decision
//! about marker, indent, colour, and stream, and they drifted into four
//! different status vocabularies. This test keeps that from coming back: if you
//! need to print something, add it to `ui`, or call an existing helper.

use std::fs;
use std::path::Path;

/// The only file allowed to call the print macros, relative to `src/`.
const SINK: &str = "ui.rs";

const MACROS: [&str; 4] = ["println!", "eprintln!", "print!", "eprint!"];

#[test]
fn no_command_prints_directly() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    walk(&src, &src, &mut offenders);

    assert!(
        offenders.is_empty(),
        "these lines print directly instead of going through `ui`:\n{}\n\n\
         Use ui::section / ok / info / warn / detail / fatal / prefixed / emit \
         instead, or add a helper to src/ui.rs.",
        offenders.join("\n"),
    );
}

fn walk(dir: &Path, root: &Path, offenders: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, root, offenders);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if rel == Path::new(SINK) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            // `print!` is a substring of `eprint!`, so match the macro call as
            // written rather than by bare containment.
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("//!") {
                continue;
            }
            for m in MACROS {
                let hit = match line.find(m) {
                    Some(0) => true,
                    Some(idx) => !line[..idx]
                        .ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.'),
                    None => false,
                };
                if hit {
                    offenders.push(format!("  {}:{}: {}", rel.display(), i + 1, code));
                    break;
                }
            }
        }
    }
}
