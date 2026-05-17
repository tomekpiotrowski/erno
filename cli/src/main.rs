mod commands;
mod global_config;
mod ng;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "erno", about = "CLI tool for the Erno framework")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        /// Local path to the erno repository root (default: uses git reference)
        #[arg(long, value_name = "PATH")]
        erno_path: Option<String>,
        /// Capacitor bundle ID (default: com.example.<name>)
        #[arg(long, value_name = "ID")]
        bundle_id: Option<String>,
    },
    /// Configure global Erno settings (~/.erno/config.toml)
    Setup,
    /// Start the api and app dev servers
    Dev,
    /// Set up and manage production deployment
    Deploy(DeployArgs),
}

#[derive(Args)]
struct DeployArgs {
    #[command(subcommand)]
    command: DeployCommands,
}

#[derive(Subcommand)]
enum DeployCommands {
    /// Generate Dockerfiles, Helm chart, and GitHub Actions workflow
    Init,
    /// Deploy a specific version to the cluster
    Install {
        /// Image tag / Helm chart version to deploy (e.g. v1.2.3)
        version: String,
        /// Target environment
        #[arg(long, default_value = "production")]
        env: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => commands::doctor::handle_doctor().await,
        Commands::New {
            name,
            path,
            erno_path,
            bundle_id,
        } => commands::new::handle_new(&name, path.as_deref(), erno_path.as_deref(), bundle_id.as_deref()).await,
        Commands::Setup => commands::setup::handle_setup().await,
        Commands::Dev => commands::dev::handle_dev(None).await,
        Commands::Deploy(args) => match args.command {
            DeployCommands::Init => commands::deploy::handle_deploy_init().await,
            DeployCommands::Install { version, env } => {
                commands::deploy::handle_deploy_install(&version, &env).await
            }
        },
    }
}
