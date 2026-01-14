use strum::{Display, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum Environment {
    #[default]
    Development,
    Production,
    Test,
}
