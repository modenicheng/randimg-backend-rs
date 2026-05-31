use migration::MigratorTrait;
use randimg_backend_rs::{auth, config::AppConfig, db};
use std::io::{self, Write};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

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

    let hash = auth::password::hash_password(password);

    let admin = db::query::admin::create(&db, username, &hash, true)
        .await
        .expect("Failed to create admin");

    println!("Created admin: {} (id={})", admin.username, admin.id);
}
