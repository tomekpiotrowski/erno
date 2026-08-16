use {{crate_name}}::{boot_config, Migrator};
use erno::boot::boot;

#[tokio::main]
async fn main() {
    boot::<Migrator, ()>(boot_config()).await;
}
