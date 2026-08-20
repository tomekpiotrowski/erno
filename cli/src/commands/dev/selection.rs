use super::DevArgs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceSelection {
    pub api: bool,
    pub app: bool,
    pub www: bool,
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
            seed: false,
            open: false,
            ios: false,
            android: false,
            target: None,
            no_prometheus: false,
            no_admin: false,
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
}
