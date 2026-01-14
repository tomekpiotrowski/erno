use std::{
    error::Error,
    process::{self, Command},
};

use crate::config::{Config, DatabaseConfig};

pub fn handle_db_console_command(config: &Config) {
    println!("🗄️  Opening database connection with psql...");

    if let Err(e) = handle_db_command(&config.database) {
        eprintln!("❌ Failed to open database connection: {e}");
        process::exit(1);
    }
}

pub fn handle_db_command(db_config: &DatabaseConfig) -> Result<(), Box<dyn Error>> {
    println!("🔗 Launching psql with database connection...");
    println!("   (Use \\q to quit, \\h for help, \\l to list databases)");
    println!();

    // Execute psql with the database URL directly
    let status = Command::new("psql").arg(&db_config.url).status()?;

    if !status.success() {
        return Err(format!("psql exited with code: {:?}", status.code()).into());
    }

    Ok(())
}
