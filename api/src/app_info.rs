/// Identifies a binary crate (app) and its version metadata.
///
/// Exists so `erno` can print version info for the concrete application
/// (e.g. the `api` binary) in addition to `erno` itself.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

impl AppInfo {
    #[must_use]
    pub const fn new(name: &'static str, version: &'static str, description: &'static str) -> Self {
        Self {
            name,
            version,
            description,
        }
    }

    #[must_use]
    pub fn api_core() -> Self {
        Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            description: env!("CARGO_PKG_DESCRIPTION"),
        }
    }
}
