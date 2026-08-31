mod commands;
mod deploy;
mod global_config;
mod ng;
mod ui;
mod version;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "erno", about = "CLI tool for the Erno framework")]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,
    #[command(subcommand)]
    command: Commands,
}

/// Accepted on either side of the subcommand, so both `erno -q build` and
/// `erno build -q` work.
#[derive(Args, Clone, Debug, Default)]
struct GlobalArgs {
    /// Disable ANSI colour (also honours NO_COLOR)
    #[arg(long, global = true)]
    no_color: bool,
    /// Disable emoji, falling back to word markers (also honours ERNO_EMOJI=0)
    #[arg(long, global = true)]
    no_emoji: bool,
    /// Print only warnings, errors, and results
    #[arg(long, short = 'q', global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Print more detail; in `dev`, stream every child log line
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Check that your environment is ready to build Erno apps
    Doctor,
    /// Scaffold a new full-stack Erno project
    New {
        /// Project name (lowercase, letters/digits/hyphens/underscores)
        name: String,
        /// Directory to create the project in (default: current directory)
        #[arg(long)]
        path: Option<String>,
        /// Local path to the erno repository root (default: this CLI's GitHub tag)
        #[arg(long, value_name = "PATH")]
        erno_path: Option<String>,
        /// Capacitor bundle ID (default: com.example.<name>)
        #[arg(long, value_name = "ID")]
        bundle_id: Option<String>,
        /// Start `erno dev` after scaffolding without prompting
        #[arg(long)]
        dev: bool,
        /// Do not start `erno dev` after scaffolding
        #[arg(long)]
        no_dev: bool,
    },
    /// Configure global Erno settings (~/.erno/config.toml)
    Setup,
    /// Start the api and app dev servers
    Dev(commands::dev::DevArgs),
    /// Build the project's packages, in dependency order
    Build(commands::packages::SelectionArgs),
    /// Format-check, lint, and typecheck the project's packages
    Lint(commands::lint::LintArgs),
    /// Run the project's test suites
    Test(commands::packages::SelectionArgs),
    /// Reset local build artifacts and databases
    Clean(commands::clean::CleanArgs),
    /// Update Erno-managed packages in this project
    Upgrade(commands::upgrade::UpgradeArgs),
    /// Set up and manage production deployment
    Deploy(DeployArgs),
    /// Register this application with a monitoring collector
    Monitoring(MonitoringArgs),
}

#[derive(Args)]
struct MonitoringArgs {
    #[command(subcommand)]
    command: commands::monitoring::MonitoringCommands,
}

#[derive(Args)]
struct DeployArgs {
    #[command(subcommand)]
    command: DeployCommands,
}

#[derive(Subcommand)]
enum DeployCommands {
    /// Generate Dockerfiles, deploy config, and GitHub Actions workflow
    Init {},
    /// Deploy a specific version to the cluster
    Install {
        /// Image tag to deploy (e.g. v1.2.3)
        version: String,
        /// Target environment
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Show what would change in the cluster
    Diff {
        /// Image tag to compare (e.g. v1.2.3)
        version: String,
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Show the recorded revision and live deployments
    Status {
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Re-install the previous revision's image tags
    Rollback {
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Convert a Helm chart/ tree into deploy/
    Migrate {},
    /// Install cert-manager and ingress-nginx (once per cluster)
    Setup {
        #[arg(long, default_value = "production")]
        env: String,
        /// Re-apply even if the add-ons are already present
        #[arg(long)]
        upgrade: bool,
        /// ingress-nginx static manifest: cloud (LoadBalancer), kind, or baremetal
        #[arg(long, value_enum)]
        provider: Option<crate::deploy::IngressProvider>,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    ui::init(
        cli.global.no_color,
        cli.global.no_emoji,
        cli.global.quiet,
        cli.global.verbose,
    );

    match dispatch(cli.command).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // `Silent` means the command already reported the details.
        Err(ui::Failure::Silent) => std::process::ExitCode::FAILURE,
        Err(ui::Failure::Message(msg)) => {
            ui::fatal(&msg);
            std::process::ExitCode::FAILURE
        }
    }
}

async fn dispatch(command: Commands) -> ui::Cmd {
    match command {
        Commands::Doctor => commands::doctor::handle_doctor().await,
        Commands::New {
            name,
            path,
            erno_path,
            bundle_id,
            dev,
            no_dev,
        } => {
            commands::new::handle_new(
                &name,
                path.as_deref(),
                erno_path.as_deref(),
                bundle_id.as_deref(),
                dev,
                no_dev,
            )
            .await
        }
        Commands::Setup => commands::setup::handle_setup().await,
        Commands::Dev(args) => commands::dev::handle_dev(None, args).await,
        Commands::Build(args) => commands::build::handle_build(args).await,
        Commands::Lint(args) => commands::lint::handle_lint(args).await,
        Commands::Test(args) => commands::test::handle_test(args).await,
        Commands::Clean(args) => commands::clean::handle_clean(args).await,
        Commands::Upgrade(args) => commands::upgrade::handle_upgrade(args).await,
        Commands::Monitoring(args) => commands::monitoring::handle(args.command).await,
        Commands::Deploy(args) => match args.command {
            DeployCommands::Init {} => {
                commands::deploy::handle_deploy_init(deploy::Target::detect()).await
            }
            DeployCommands::Install { version, env } => {
                commands::deploy::handle_deploy_install(&version, &env, deploy::Target::detect())
                    .await
            }
            DeployCommands::Diff { version, env } => {
                crate::deploy::handle_diff(&version, &env, deploy::Target::detect()).await
            }
            DeployCommands::Status { env } => {
                crate::deploy::handle_status(&env, deploy::Target::detect()).await
            }
            DeployCommands::Rollback { env } => {
                crate::deploy::handle_rollback(&env, deploy::Target::detect()).await
            }
            DeployCommands::Migrate {} => crate::deploy::handle_migrate(deploy::Target::detect()),
            DeployCommands::Setup {
                env,
                upgrade,
                provider,
            } => crate::deploy::handle_setup(&env, deploy::Target::detect(), upgrade, provider),
        },
    }
}
