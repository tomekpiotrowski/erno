use thiserror::Error;

#[derive(Error, Debug)]
pub enum UniqueConstraintError {
    #[error("The value for field '{0}' must be unique.")]
    UniquenessError(&'static str),
    #[error("Database error: {0}")]
    Other(sea_orm::DbErr),
}

/// Maps database unique constraint violations to uniqueness errors
pub fn handle_unique_constraint_violation(
    field_name: &'static str,
    index_name: &'static str,
) -> impl Fn(sea_orm::DbErr) -> UniqueConstraintError {
    move |db_err: sea_orm::DbErr| {
        let error_message = db_err.to_string();
        if error_message.contains("duplicate key value violates unique constraint")
            && error_message.contains(&index_name.to_string())
        {
            UniqueConstraintError::UniquenessError(field_name)
        } else {
            UniqueConstraintError::Other(db_err)
        }
    }
}
