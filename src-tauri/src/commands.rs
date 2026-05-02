use crate::{SharedConfig, SharedConfigManager};
use rl_stats_core::{AppConfig, AppState, StateReceiver};

/// Get the current application configuration.
/// 
/// # Errors
/// Returns an error if the configuration lock cannot be acquired.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_config(config: tauri::State<'_, SharedConfig>) -> Result<AppConfig, String> {
    let guard = config
        .lock()
        .map_err(|_| "failed to acquire config lock".to_string())?;
    Ok(guard.clone())
}

/// Save the current application configuration.
/// 
/// # Errors
/// Returns an error if the configuration lock cannot be acquired or if saving the configuration fails.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn save_config(
    new_config: AppConfig,
    config: tauri::State<'_, SharedConfig>,
    config_manager: tauri::State<'_, SharedConfigManager>,
) -> Result<(), String> {
    config_manager
        .save(&new_config)
        .map_err(|err| format!("failed to save config: {err}"))?;

    let mut guard = config
        .lock()
        .map_err(|_| "failed to acquire config lock".to_string())?;
    *guard = new_config;

    Ok(())
}

#[tauri::command]
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn get_status(state: tauri::State<'_, StateReceiver>) -> AppState {
    state.borrow().clone()
}
