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
    #[serde(rename = "Firebase")]
    Firebase {
        url: String,
        #[serde(rename = "apiKey", alias = "api_key")]
        api_key: String,
        email: String,
        #[serde(default)]
        password: Option<String>,
    },
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self::Firebase {
            url: "https://your-project.firebaseio.com".to_string(),
            api_key: "your-firebase-web-api-key".to_string(),
            email: "firebase-user@example.com".to_string(),
            password: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(alias = "is_headless")]
    pub is_headless: bool,
    #[serde(alias = "websocket_url")]
    pub websocket_url: String,
    #[serde(default)]
    pub connector: ConnectorConfig,
    #[serde(alias = "reconnect_delay_seconds")]
    pub reconnect_delay_seconds: u64,
    #[serde(default = "default_ui_sync_port", alias = "ui_sync_port")]
    pub ui_sync_port: u16,
    #[serde(default, alias = "remember_password")]
    pub remember_password: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            is_headless: false,
            websocket_url: "ws://127.0.0.1:49123".to_string(),
            connector: ConnectorConfig::default(),
            reconnect_delay_seconds: 5,
            ui_sync_port: default_ui_sync_port(),
            remember_password: false,
        }
    }
}

impl AppConfig {
    #[must_use]
    pub fn sanitized_for_storage(&self) -> Self {
        if self.remember_password {
            return self.clone();
        }

        let mut sanitized = self.clone();
        match &mut sanitized.connector {
            ConnectorConfig::Firebase { password, .. } => {
                *password = None;
            }
        }

        sanitized
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

        let sanitized = config.sanitized_for_storage();
        let json = serde_json::to_string_pretty(&sanitized)?;
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

#[cfg(test)]
mod tests {
    use super::{AppConfig, default_ui_sync_port};
    use serde_json::Error as JsonError;

    #[test]
    fn app_config_deserializes_without_ui_sync_port() -> Result<(), JsonError> {
        let legacy_json = r#"{
            "is_headless": false,
            "websocket_url": "ws://127.0.0.1:49123",
            "connector": {
                "type": "Firebase",
                "url": "https://your-project.firebaseio.com",
                "api_key": "test-api-key",
                "email": "firebase-user@example.com",
                "password": "secret"
            },
            "reconnect_delay_seconds": 5
        }"#;

        let config: AppConfig = serde_json::from_str(legacy_json)?;

        assert_eq!(config.ui_sync_port, default_ui_sync_port());
        assert!(!config.remember_password);
        Ok(())
    }

    #[test]
    fn app_config_deserializes_with_explicit_ui_sync_port() -> Result<(), JsonError> {
        let json = r#"{
            "is_headless": false,
            "websocket_url": "ws://127.0.0.1:49123",
            "connector": {
                "type": "Firebase",
                "url": "https://your-project.firebaseio.com",
                "api_key": "test-api-key",
                "email": "firebase-user@example.com",
                "password": "secret"
            },
            "reconnect_delay_seconds": 5,
            "ui_sync_port": 60000
        }"#;

        let config: AppConfig = serde_json::from_str(json)?;

        assert_eq!(config.ui_sync_port, 60000);
        assert!(!config.remember_password);
        Ok(())
    }

    #[test]
    fn app_config_deserializes_with_missing_password() -> Result<(), JsonError> {
        let json = r#"{
            "is_headless": false,
            "websocket_url": "ws://127.0.0.1:49123",
            "connector": {
                "type": "Firebase",
                "url": "https://your-project.firebaseio.com",
                "api_key": "test-api-key",
                "email": "firebase-user@example.com"
            },
            "reconnect_delay_seconds": 5,
            "ui_sync_port": 54321,
            "remember_password": false
        }"#;

        let config: AppConfig = serde_json::from_str(json)?;
        match config.connector {
            super::ConnectorConfig::Firebase { password, .. } => {
                assert_eq!(password, None);
            }
        }
        Ok(())
    }

    #[test]
    fn sanitized_for_storage_removes_password_when_not_remembered() {
        let config = AppConfig {
            remember_password: false,
            connector: super::ConnectorConfig::Firebase {
                url: "https://your-project.firebaseio.com".to_string(),
                api_key: "test-api-key".to_string(),
                email: "firebase-user@example.com".to_string(),
                password: Some("secret".to_string()),
            },
            ..AppConfig::default()
        };

        let sanitized = config.sanitized_for_storage();
        match sanitized.connector {
            super::ConnectorConfig::Firebase { password, .. } => {
                assert_eq!(password, None);
            }
        }
    }
}
