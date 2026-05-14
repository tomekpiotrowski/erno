use sea_orm::DeriveActiveEnum;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, EnumString};

#[derive(
    Debug, Clone, PartialEq, Eq, DeriveActiveEnum, Serialize, Deserialize, EnumIter, EnumString,
)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "subscription_status")]
pub enum SubscriptionStatus {
    #[sea_orm(string_value = "active")]
    Active,
    #[sea_orm(string_value = "past_due")]
    PastDue,
    #[sea_orm(string_value = "canceled")]
    Canceled,
}
