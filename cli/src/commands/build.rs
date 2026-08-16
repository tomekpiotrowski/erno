use crate::commands::dev::resolve_project_root;
use crate::commands::packages::{load_packages, run_phase, select, Phase, SelectionArgs};

pub async fn handle_build(args: SelectionArgs) {
    let root = resolve_project_root(None);
    let all = match load_packages(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };
    let selected = match select(&all, &args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };

    let ok = run_phase(&root, &selected, Phase::Build, false, &args, &mut |_| None);
    if !ok {
        std::process::exit(1);
    }
}
