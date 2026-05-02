use crate::{AppConfig, AppState, SharedAppState, SharedConfig, SharedConfigManager};

/// Get the current application configuration.
/// 
/// # Errors
/// Returns an error if the configuration lock cannot be acquired.
#[tauri::command]
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
pub fn save_config(
    new_config: AppConfig,
    config: tauri::State<'_, SharedConfig>,
    config_manager: tauri::State<'_, SharedConfigManager>,
) -> Result<(), String> {
    config_manager
        .save(&new_config)
        .map_err(|err| format!("failed to save config: {}", err))?;

    let mut guard = config
        .lock()
        .map_err(|_| "failed to acquire config lock".to_string())?;
    *guard = new_config;

    Ok(())
}

/// Get the current application status.
/// 
/// # Errors
/// Returns an error if the application state lock cannot be acquired.
#[tauri::command]
pub fn get_status(state: tauri::State<'_, SharedAppState>) -> Result<AppState, String> {
    let guard = state
        .lock()
        .map_err(|_| "failed to acquire app state lock".to_string())?;
    Ok(guard.clone())
}
