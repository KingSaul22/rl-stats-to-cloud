use crate::{AppConfig, AppState, SharedAppState, SharedConfig, SharedConfigManager};

#[tauri::command]
pub fn get_config(config: tauri::State<'_, SharedConfig>) -> Result<AppConfig, String> {
    let guard = config
        .lock()
        .map_err(|_| "failed to acquire config lock".to_string())?;
    Ok(guard.clone())
}

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

#[tauri::command]
pub fn get_status(state: tauri::State<'_, SharedAppState>) -> Result<AppState, String> {
    let guard = state
        .lock()
        .map_err(|_| "failed to acquire app state lock".to_string())?;
    Ok(guard.clone())
}
