use std::process;

use crate::{console::RhaiConsole, environment::Environment};

pub fn handle_console_command(environment: Environment) {
    println!("🧩 Starting Rhai console...");

    // Create database connection for console
    let mut console = RhaiConsole::new(environment);

    if let Err(e) = console.start_interactive() {
        eprintln!("Console error: {e}");
        process::exit(1);
    }
}
