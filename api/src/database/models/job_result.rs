use sea_orm::sea_query::StringLen;
use sea_orm::DeriveActiveEnum;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
    EnumIter,
    EnumString,
    Display,
)]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::N(32))",
    enum_name = "job_result"
)]
pub enum JobResult {
    #[sea_orm(string_value = "completed")]
    Completed,
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "timed_out")]
    TimedOut,
}

#[allow(dead_code)]
impl JobResult {
    pub const fn is_successful(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    pub const fn is_timed_out(&self) -> bool {
        matches!(self, Self::TimedOut)
    }
}
