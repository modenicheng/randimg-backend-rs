use migration::MigratorTrait;
use randimg_core::{auth, config::AppConfig, db};
use std::io::{self, Write};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // CLI 工具也使用 tracing，输出到 stderr，不影响交互式 stdin/stdout
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env();

    let db = sea_orm::Database::connect(&config.database_url)
        .await
        .expect("Failed to connect to database");

    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");

    print!("Username: ");
    io::stdout().flush().unwrap();
    let mut username = String::new();
    io::stdin().read_line(&mut username).unwrap();
    let username = username.trim();

    print!("Password: ");
    io::stdout().flush().unwrap();
    let mut password = String::new();
    io::stdin().read_line(&mut password).unwrap();
    let password = password.trim();

    let hash = auth::password::hash_password(password).expect("Password hashing failed");

    let admin = db::query::admin::create(&db, username, &hash, true)
        .await
        .expect("Failed to create admin");

    tracing::info!(username = %admin.username, admin_id = admin.id, "Admin created");
}
