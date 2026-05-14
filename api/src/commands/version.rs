use std::env;

use crate::app_info::AppInfo;

pub fn print_version_info(app: AppInfo) {
    let core = AppInfo::api_core();

    // Get build information if available
    let git_hash = option_env!("GIT_HASH").unwrap_or("unknown");
    let build_timestamp = option_env!("BUILD_TIMESTAMP").unwrap_or("unknown");
    let rustc_version = option_env!("RUSTC_VERSION").unwrap_or("unknown");

    println!("📦 {} v{}", app.name, app.version);

    if !app.description.is_empty() {
        println!("📝 {}", app.description);
    }

    println!("🧱 Uses {} v{}", core.name, core.version);

    if !core.description.is_empty() {
        println!("📝 erno description: {}", core.description);
    }

    println!();
    println!("🔨 Build Information:");
    println!("  🔗 Git Hash: {git_hash}");
    println!("  ⏰ Build Time: {build_timestamp}");
    println!("  🦀 Rust Version: {rustc_version}");
    println!();

    // Runtime information
    println!("💻 Runtime Information:");
    println!("  🖥️  OS: {}", env::consts::OS);
    println!("  🏗️  Architecture: {}", env::consts::ARCH);
}
