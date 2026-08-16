use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::{load_packages, run_phase, select, Phase, SelectionArgs};

#[derive(Args, Debug, Default)]
pub struct LintArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,
    /// Apply fixes instead of only reporting them
    #[arg(long)]
    pub fix: bool,
}

pub async fn handle_lint(args: LintArgs) {
    let root = resolve_project_root(None);
    let all = match load_packages(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };
    let selected = match select(&all, &args.selection) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };

    let ok = run_phase(
        &root,
        &selected,
        Phase::Lint,
        args.fix,
        &args.selection,
        &mut |_| None,
    );
    if !ok {
        std::process::exit(1);
    }
}
