use crate::bridge::request_poweroff;
use crate::{SharedConfig, SharedConfigManager};
use rl_stats_core::{AppConfig, AppState, StateReceiver};

/// Get the current app configuration from the shared config manager.
///
/// # Errors
/// Returns an error if config loading or creation fails.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_config(
    config: tauri::State<'_, SharedConfigManager>,
) -> Result<AppConfig, String> {
    config
        .load_or_create()
        .map_err(|err| format!("failed to load config: {err}"))
}

/// Persist configuration and update the shared in-memory config snapshot.
///
/// # Errors
/// Returns an error if save fails or the shared config lock cannot be acquired.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn save_config(
    new_config: AppConfig,
    config: tauri::State<'_, SharedConfigManager>,
    shared_config: tauri::State<'_, SharedConfig>,
) -> Result<(), String> {
    config
        .save(&new_config)
        .map_err(|err| format!("failed to save config: {err}"))?;

    // Runtime note: the daemon currently wires sink/transport config at startup.
    // Frontend flows must trigger a daemon restart mechanism for connector or
    // websocket URL changes to take effect.
    println!(
        "Config saved. A daemon restart is required to apply connector/websocket URL changes."
    );

    let mut guard = shared_config
        .lock()
        .map_err(|_| "failed to acquire config lock".to_string())?;
    *guard = new_config;
    drop(guard);

    Ok(())
}

/// Retrieve the latest daemon-derived app status.
///
/// # Errors
/// This command currently does not produce runtime errors.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_status(state: tauri::State<'_, StateReceiver>) -> Result<AppState, String> {
    Ok(state.borrow().clone())
}

/// Request daemon shutdown through the control plane.
///
/// # Errors
/// Returns an error if the daemon is not reachable or rejects the poweroff command.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn shutdown_daemon() -> Result<(), String> {
    request_poweroff().await
}
