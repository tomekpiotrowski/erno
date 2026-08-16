use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::{load_packages, run_phase, select, Phase, SelectionArgs};
use crate::ui;

#[derive(Args, Debug, Default)]
pub struct LintArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,
    /// Apply fixes instead of only reporting them
    #[arg(long)]
    pub fix: bool,
}

pub async fn handle_lint(args: LintArgs) -> ui::Cmd {
    let root = resolve_project_root(None)?;
    let all = load_packages(&root)?;
    let selected = select(&all, &args.selection)?;

    if run_phase(
        &root,
        &selected,
        Phase::Lint,
        args.fix,
        &args.selection,
        &mut |_| None,
    ) {
        Ok(())
    } else {
        // `run_phase` already printed the per-package summary.
        Err(ui::Failure::Silent)
    }
}
