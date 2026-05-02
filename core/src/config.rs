use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub type ConfigResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const APP_CONFIG_DIR_NAME: &str = "com.kingsaul22.rlstatscloud";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConnectorConfig {
    Firebase {
        url: String,
        auth_token: Option<String>,
    },
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self::Firebase {
            url: "https://your-project.firebaseio.com".to_string(),
            auth_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub is_headless: bool,
    pub websocket_url: String,
    #[serde(default)]
    pub connector: ConnectorConfig,
    pub reconnect_delay_seconds: u64,
    #[serde(default = "default_ui_sync_port")]
    pub ui_sync_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            is_headless: false,
            websocket_url: "ws://127.0.0.1:49123".to_string(),
            connector: ConnectorConfig::default(),
            reconnect_delay_seconds: 5,
            ui_sync_port: default_ui_sync_port(),
        }
    }
}

#[must_use]
pub const fn default_ui_sync_port() -> u16 {
    54_321
}

#[derive(Debug, Clone)]
pub struct ConfigManager {
    path: PathBuf,
}

impl ConfigManager {
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn local() -> Self {
        Self::new(default_config_path())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the configuration from the file if it exists, otherwise creates a new file with default settings and returns that.
    ///
    /// # Errors
    /// Returns an error if reading from or writing to the file fails, or if the file contents cannot be parsed as JSON.
    pub fn load_or_create(&self) -> ConfigResult<AppConfig> {
        if self.path.exists() {
            return self.load();
        }

        let config = AppConfig::default();
        self.save(&config)?;
        Ok(config)
    }

    /// Loads the configuration from the file.
    ///
    /// # Errors
    /// Returns an error if reading from the file fails or if the file contents cannot be parsed as JSON.
    pub fn load(&self) -> ConfigResult<AppConfig> {
        let content = fs::read_to_string(&self.path)?;
        let config: AppConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Saves the provided configuration to the file, creating any necessary parent directories.
    ///
    /// # Errors
    /// Returns an error if creating parent directories fails, if serializing the configuration to JSON fails, or if writing to the file fails.
    pub fn save(&self, config: &AppConfig) -> ConfigResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(config)?;
        fs::write(&self.path, json)?;
        Ok(())
    }
}

#[must_use]
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
