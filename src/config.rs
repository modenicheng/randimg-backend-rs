use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub database_url: String,
    pub secret_key: String,
    pub jwt_expire_minutes: u64,
    pub cdn_base_url: String,
    pub image_dir: String,
    pub server_addr: String,
    pub pixiv_refresh_token: String,
    pub pixiv_proxy: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let secret_key = env::var("SECRET_KEY")
            .unwrap_or_else(|_| "change-me".into());
        if secret_key == "change-me" {
            panic!("SECRET_KEY must be set in environment. Do not use the default 'change-me'.");
        }

        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/randimg.db?mode=rune".into()),
            secret_key,
            jwt_expire_minutes: env::var("JWT_EXPIRE_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            cdn_base_url: env::var("CDN_BASE_URL")
                .unwrap_or_else(|_| "https://cdn.example.com/".into()),
            image_dir: env::var("IMAGE_DIR")
                .unwrap_or_else(|_| "./images".into()),
            server_addr: env::var("SERVER_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8000".into()),
            pixiv_refresh_token: env::var("PIXIV_REFRESH_TOKEN")
                .unwrap_or_default(),
            pixiv_proxy: env::var("PIXIV_PROXY")
                .unwrap_or_default(),
        }
    }
}
