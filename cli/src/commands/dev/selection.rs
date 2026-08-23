use super::DevArgs;
use crate::commands::packages::Package;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceSelection {
    pub api: bool,
    pub app: bool,
    pub www: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraService {
    pub name: String,
    pub dir: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
}

/// Extra `[[package.dev]]` processes to start, in declaration order.
///
/// `--package` / `--all` only affect these; `--api` / `--app` / `--www` stay
/// conventional. Naming a package does not pull a `default = false` step
/// unless `--all` is also passed.
pub fn extra_services(
    packages: &[Package],
    named: &[String],
    all: bool,
) -> Result<Vec<ExtraService>, String> {
    if !named.is_empty() {
        let known: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        for asked in named {
            if !known.contains(&asked.as_str()) {
                return Err(format!(
                    "unknown package '{asked}'. Known: {}",
                    known.join(", ")
                ));
            }
            let pkg = packages.iter().find(|p| p.name == *asked).unwrap();
            if pkg.dev.is_empty() {
                return Err(format!("package '{asked}' has no [[package.dev]]"));
            }
        }
    }

    let mut out = Vec::new();
    for pkg in packages {
        let selected = if !named.is_empty() {
            named.iter().any(|n| n == &pkg.name)
        } else if all {
            true
        } else {
            pkg.default
        };
        if !selected {
            continue;
        }
        for step in &pkg.dev {
            if step.default || all {
                out.push(ExtraService {
                    name: pkg.name.clone(),
                    dir: pkg.dir.clone(),
                    command: step.command.clone(),
                    args: step.args.clone(),
                    url: step.url.clone(),
                });
            }
        }
    }
    Ok(out)
}

impl ServiceSelection {
    pub fn resolve(args: &DevArgs, has_www: bool) -> Result<Self, String> {
        if args.www && args.no_www {
            return Err("cannot combine --www and --no-www".into());
        }
        if args.ios && args.android {
            return Err("cannot combine --ios and --android".into());
        }

        let device = args.ios || args.android;
        let explicit = args.api || args.app || args.www;
        let sel = Self {
            api: if device {
                true
            } else if explicit {
                args.api
            } else {
                true
            },
            app: if device {
                true
            } else if explicit {
                args.app
            } else {
                true
            },
            www: if args.no_www {
                false
            } else if explicit {
                args.www
            } else {
                has_www
            },
        };

        if !sel.api && !sel.app && !sel.www {
            return Err("nothing to start — pass --api, --app, and/or --www".into());
        }
        if sel.www && !has_www {
            return Err("no www/ directory with a package.json".into());
        }
        Ok(sel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(api: bool, app: bool, www: bool, no_www: bool) -> DevArgs {
        DevArgs {
            api,
            app,
            www,
            no_www,
            ..DevArgs::default()
        }
    }

    #[test]
    fn default_starts_everything_present() {
        let sel = ServiceSelection::resolve(&args(false, false, false, false), true).unwrap();
        assert_eq!(
            sel,
            ServiceSelection {
                api: true,
                app: true,
                www: true
            }
        );
        let sel = ServiceSelection::resolve(&args(false, false, false, false), false).unwrap();
        assert!(!sel.www);
    }

    #[test]
    fn no_www_skips_marketing() {
        let sel = ServiceSelection::resolve(&args(false, false, false, true), true).unwrap();
        assert!(sel.api && sel.app && !sel.www);
    }

    #[test]
    fn explicit_api_only() {
        let sel = ServiceSelection::resolve(&args(true, false, false, false), true).unwrap();
        assert_eq!(
            sel,
            ServiceSelection {
                api: true,
                app: false,
                www: false
            }
        );
    }

    #[test]
    fn rejects_www_without_directory() {
        assert!(ServiceSelection::resolve(&args(false, false, true, false), false).is_err());
    }

    #[test]
    fn rejects_www_and_no_www() {
        assert!(ServiceSelection::resolve(&args(false, false, true, true), true).is_err());
    }

    #[test]
    fn device_flags_force_api_and_app() {
        let mut a = args(false, false, true, false);
        a.ios = true;
        let sel = ServiceSelection::resolve(&a, true).unwrap();
        assert!(sel.api && sel.app && sel.www);

        let mut both = args(false, false, false, false);
        both.ios = true;
        both.android = true;
        assert!(ServiceSelection::resolve(&both, false).is_err());
    }

    use crate::commands::packages::{DevService, Package};

    fn pkg(name: &str, default: bool, dev: Vec<DevService>) -> Package {
        Package {
            name: name.into(),
            dir: name.into(),
            default,
            database: false,
            kind: None,
            build: vec![],
            lint: vec![],
            test: vec![],
            dev,
        }
    }

    fn dev(command: &str, url: &str, default: bool) -> DevService {
        DevService {
            command: command.into(),
            args: vec![],
            url: url.into(),
            default,
        }
    }

    fn extras() -> Vec<Package> {
        vec![
            pkg("app", true, vec![]),
            pkg(
                "vision",
                false,
                vec![dev(
                    "./serve.sh",
                    "http://localhost:8765/tools/solve_studio/",
                    true,
                )],
            ),
        ]
    }

    #[test]
    fn default_run_skips_opt_in_packages() {
        assert!(extra_services(&extras(), &[], false).unwrap().is_empty());
    }

    #[test]
    fn default_package_with_default_dev_starts() {
        let packages = vec![pkg(
            "studio",
            true,
            vec![dev("./s", "http://localhost:1", true)],
        )];
        assert_eq!(extra_services(&packages, &[], false).unwrap().len(), 1);
    }

    #[test]
    fn package_flag_adds_the_named_extra() {
        let got = extra_services(&extras(), &["vision".into()], false).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "vision");
        assert_eq!(got[0].command, "./serve.sh");
        assert_eq!(got[0].url, "http://localhost:8765/tools/solve_studio/");
        // Conventional flags are independent: --api still means only the API.
        let sel = ServiceSelection::resolve(&args(true, false, false, false), true).unwrap();
        assert!(sel.api && !sel.app && !sel.www);
    }

    #[test]
    fn all_includes_opt_in_packages() {
        let got = extra_services(&extras(), &[], true).unwrap();
        assert_eq!(
            got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["vision"]
        );
    }

    #[test]
    fn unknown_package_lists_known() {
        let err = extra_services(&extras(), &["nope".into()], false).unwrap_err();
        assert!(err.contains("unknown package 'nope'"), "{err}");
        assert!(err.contains("app"), "{err}");
        assert!(err.contains("vision"), "{err}");
    }

    #[test]
    fn named_package_without_dev_is_an_error() {
        let err = extra_services(&extras(), &["app".into()], false).unwrap_err();
        assert!(err.contains("no [[package.dev]]"), "{err}");
    }

    #[test]
    fn naming_a_package_does_not_pull_a_non_default_step() {
        let packages = vec![pkg(
            "slow",
            true,
            vec![dev("./s", "http://localhost:1", false)],
        )];
        assert!(extra_services(&packages, &["slow".into()], false)
            .unwrap()
            .is_empty());
        let got = extra_services(&packages, &["slow".into()], true).unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn all_with_no_dev_services_is_ok() {
        let packages = vec![pkg("app", true, vec![])];
        assert!(extra_services(&packages, &[], true).unwrap().is_empty());
    }
}
