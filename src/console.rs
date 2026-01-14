use rhai::{Dynamic, Engine, Scope};
use rustyline::{error::ReadlineError, DefaultEditor, Result as RustyResult};
use tracing::{error, info, warn};

use crate::environment::Environment;

pub struct RhaiConsole {
    engine: Engine,
    environment: Environment,
}

impl RhaiConsole {
    #[must_use]
    pub fn new(environment: Environment) -> Self {
        let mut engine = Engine::new();

        // Register basic functions
        Self::register_logging_functions(&mut engine);
        Self::register_utility_functions(&mut engine);

        Self {
            engine,
            environment,
        }
    }

    pub fn start_interactive(&mut self) -> RustyResult<()> {
        println!("🧩 Rhai Console");
        println!("Environment: {:?}", self.environment);
        println!("Type 'help' for available commands, 'exit' to quit");
        println!("Rhai documentation: https://rhai.rs/book/");
        println!();

        let mut rl = DefaultEditor::new()?;
        let mut scope = Scope::new();

        // Set up initial scope variables
        scope.push("env", format!("{:?}", self.environment));

        loop {
            let readline = rl.readline("rhai> ");

            match readline {
                Ok(line) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    rl.add_history_entry(line)?;
                    match line {
                        "exit" | "quit" => {
                            println!("Goodbye! 👋");
                            break;
                        }
                        "help" => {
                            Self::show_help();
                        }
                        "clear" => {
                            print!("\x1B[2J\x1B[1;1H"); // Clear screen
                        }
                        _ => {
                            self.execute_rhai_code(line, &mut scope);
                        }
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C pressed, exiting...");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D pressed, exiting...");
                    break;
                }
                Err(err) => {
                    error!("Error reading line: {:?}", err);
                    break;
                }
            }
        }

        Ok(())
    }

    fn execute_rhai_code(&self, code: &str, scope: &mut Scope<'_>) {
        match self.engine.eval_with_scope::<Dynamic>(scope, code) {
            Ok(result) => {
                // Only print if result is not unit type
                if !result.is_unit() {
                    println!("=> {result}");
                }
            }
            Err(e) => {
                error!("Rhai error: {}", e);
            }
        }
    }

    fn show_help() {
        println!("🧩 Rhai Console");
        println!();
        println!("Built-in Commands:");
        println!("  help          - Show this help message");
        println!("  clear         - Clear the screen");
        println!("  exit/quit     - Exit the console");
        println!();
        println!("Available Functions:");
        println!("  info(msg)     - Log info message");
        println!("  warn(msg)     - Log warning message");
        println!("  error(msg)    - Log error message");
        println!("  now()         - Current timestamp");
        println!("  today()       - Start of today");
        println!();
        println!("Variables:");
        println!("  env           - Current environment");
        println!();
        println!("Examples:");
        println!("  info(\"Hello from Rhai!\");");
        println!("  let x = 42; print(x);");
        println!("  print(\"Environment: \" + env);");
        println!();
    }

    fn register_logging_functions(engine: &mut Engine) {
        engine.register_fn("info", |msg: &str| {
            info!("{}", msg);
        });

        engine.register_fn("warn", |msg: &str| {
            warn!("{}", msg);
        });

        engine.register_fn("error", |msg: &str| {
            error!("{}", msg);
        });
    }

    fn register_utility_functions(engine: &mut Engine) {
        engine.register_fn("now", || chrono::Utc::now().timestamp());

        engine.register_fn("today", || {
            chrono::Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        });
    }
}
