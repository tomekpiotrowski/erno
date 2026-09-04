//! Shared request-spec helpers. See the Erno Testing guide.

pub use erno::tests::{no_fixtures, setup_test, TestUtils};

use {{crate_name}}::boot_config;
use {{crate_name}}::Migrator;

pub async fn setup() -> TestUtils {
    setup_test::<Migrator, _>(boot_config(), no_fixtures).await
}
