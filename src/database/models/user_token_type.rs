use sea_orm::DeriveActiveEnum;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, EnumString};

#[derive(
    Debug, Clone, PartialEq, Eq, DeriveActiveEnum, Serialize, Deserialize, EnumIter, EnumString,
)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "user_token_type")]
pub enum UserTokenType {
    #[sea_orm(string_value = "email_verification")]
    EmailVerification,
    #[sea_orm(string_value = "password_reset")]
    PasswordReset,
}
