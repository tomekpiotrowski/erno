//! The version this CLI was built as, and the GitHub coordinates consumers pin.

pub const ERNO_GIT: &str = "https://github.com/tomekpiotrowski/erno";

pub fn erno_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn erno_tag() -> String {
    format!("v{}", erno_version())
}

pub fn erno_git_dep() -> String {
    format!(r#"{{ git = "{ERNO_GIT}", tag = "{}" }}"#, erno_tag())
}

pub fn erno_angular_tarball_url() -> String {
    let v = erno_version();
    format!("{ERNO_GIT}/releases/download/v{v}/erno-angular-{v}.tgz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_dep_pins_this_cli_tag() {
        let dep = erno_git_dep();
        assert!(dep.contains(ERNO_GIT), "{dep}");
        assert!(dep.contains(&format!("tag = \"{}\"", erno_tag())), "{dep}");
    }

    #[test]
    fn tarball_url_matches_this_cli_version() {
        let url = erno_angular_tarball_url();
        let v = erno_version();
        assert_eq!(
            url,
            format!("{ERNO_GIT}/releases/download/v{v}/erno-angular-{v}.tgz")
        );
    }
}
