use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub type ConfigResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const APP_CONFIG_DIR_NAME: &str = "com.kingsaul22.rlstatscloud.app";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub is_headless: bool,
    pub websocket_url: String,
    pub firebase_url: String,
    pub firebase_auth_token: Option<String>,
    pub reconnect_delay_seconds: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            is_headless: false,
            websocket_url: "ws://127.0.0.1:49123".to_string(),
            firebase_url: "https://your-project.firebaseio.com".to_string(),
            firebase_auth_token: None,
            reconnect_delay_seconds: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigManager {
    path: PathBuf,
}

impl ConfigManager {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn local() -> Self {
        Self::new(default_config_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_create(&self) -> ConfigResult<AppConfig> {
        if self.path.exists() {
            return self.load();
        }

        let config = AppConfig::default();
        self.save(&config)?;
        Ok(config)
    }

    pub fn load(&self) -> ConfigResult<AppConfig> {
        let content = fs::read_to_string(&self.path)?;
        let config: AppConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, config: &AppConfig) -> ConfigResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(config)?;
        fs::write(&self.path, json)?;
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata)
                .join(APP_CONFIG_DIR_NAME)
                .join(CONFIG_FILE_NAME);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg_config_home) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg_config_home)
                .join(APP_CONFIG_DIR_NAME)
                .join(CONFIG_FILE_NAME);
        }

        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join(APP_CONFIG_DIR_NAME)
                .join(CONFIG_FILE_NAME);
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(CONFIG_FILE_NAME)
}
