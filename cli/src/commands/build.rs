use crate::commands::dev::resolve_project_root;
use crate::commands::packages::{load_packages, run_phase, select, Phase, SelectionArgs};
use crate::ui;

pub async fn handle_build(args: SelectionArgs) -> ui::Cmd {
    let root = resolve_project_root(None)?;
    let all = load_packages(&root)?;
    let selected = select(&all, &args)?;

    if run_phase(&root, &selected, Phase::Build, false, &args, &mut |_| None) {
        Ok(())
    } else {
        // `run_phase` already printed the per-package summary.
        Err(ui::Failure::Silent)
    }
}
