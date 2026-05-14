use sea_orm::DatabaseConnection;

use crate::config::StripeConfig;

pub async fn handle_admin_command(db: DatabaseConnection, stripe: Option<StripeConfig>) {
    let plans: Vec<String> = stripe
        .as_ref()
        .map(|s| s.price_ids.keys().cloned().collect())
        .unwrap_or_default();

    tokio::task::block_in_place(move || {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = crate::admin::run(&db, &plans, &handle) {
            eprintln!("Admin TUI error: {e}");
        }
    });
}
