use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::ui;

#[derive(Args, Debug, Default)]
pub struct CleanArgs {
    /// Print the plan and exit
    #[arg(long)]
    pub dry_run: bool,
    /// Run without prompting
    #[arg(long)]
    pub yes: bool,
}

pub async fn handle_clean(_args: CleanArgs) -> ui::Cmd {
    let _root = resolve_project_root(None)?;
    ui::section(ui::icon::CLEAN, "Clean");
    Ok(())
}
