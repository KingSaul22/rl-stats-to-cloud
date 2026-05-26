// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rl_stats_core::{AppConfig, ConfigManager, default_config_path};

fn main() {
    let config_manager = ConfigManager::new(default_config_path());

    let config = match config_manager.load_or_create() {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "Failed to load config at {}: {}. Falling back to defaults.",
                config_manager.path().display(),
                err
            );
            AppConfig::default()
        }
    };

    if let Err(err) = rl_stats_to_cloud_lib::run_tauri(config) {
        eprintln!("Tauri app terminated with error: {err}");
    }
}
