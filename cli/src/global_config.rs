use std::path::PathBuf;

use config_rs::{Config, File};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub postgres: PostgresConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub admin_url: String,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            postgres: PostgresConfig {
                admin_url: "postgres://erno:erno@localhost:5432/postgres".to_string(),
            },
        }
    }
}

impl GlobalConfig {
    pub fn path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".erno").join("config.toml"))
    }

    pub fn load() -> Result<Self, config_rs::ConfigError> {
        let path = Self::path().ok_or_else(|| {
            config_rs::ConfigError::NotFound("could not determine home directory".to_string())
        })?;

        Config::builder()
            .add_source(File::from(path).required(true))
            .build()?
            .try_deserialize()
    }

    pub fn exists() -> bool {
        Self::path().map(|p| p.exists()).unwrap_or(false)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not determine home directory",
            )
        })?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = format!("[postgres]\nadmin_url = {:?}\n", self.postgres.admin_url);
        std::fs::write(path, content)
    }
}
