use crate::commands;
use rl_stats_core::{AppConfig, AppState, ConfigManager, StateReceiver, StateSender};
use std::fs;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
use tauri::async_runtime::JoinHandle;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

mod lifecycle;
mod transport;

use lifecycle::{shutdown_ui_bridge_and_disallow, spawn_ui_bridge_task};

pub type SharedConfig = Arc<Mutex<AppConfig>>;
pub type SharedConfigManager = Arc<ConfigManager>;
pub type BridgeTaskHandle = JoinHandle<()>;
pub type SharedBridgeTask = Arc<Mutex<Option<BridgeTaskHandle>>>;

pub(crate) async fn request_poweroff() -> Result<(), String> {
    transport::request_poweroff().await
}

pub(crate) async fn request_provide_password(password: String) -> Result<(), String> {
    transport::request_provide_password(password).await
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

/// Run the Tauri application with the provided configuration.
///
/// # Errors
/// Returns a `tauri::Error` if building or running the Tauri application fails.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run_tauri(config: AppConfig) -> Result<(), tauri::Error> {
    let (state_sender, state_receiver): (StateSender, StateReceiver) =
        watch::channel(AppState::default());
    let ui_sync_port = config.ui_sync_port;
    let shared_config: SharedConfig = Arc::new(Mutex::new(config));
    let shutdown = CancellationToken::new();
    let is_shutting_down = Arc::new(AtomicBool::new(false));
    let bridge_task: SharedBridgeTask = Arc::new(Mutex::new(None));

    let setup_shutdown = shutdown.clone();
    let setup_state_sender = state_sender;
    let setup_bridge_task = Arc::clone(&bridge_task);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state_receiver)
        .manage(Arc::clone(&shared_config))
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_config,
            commands::save_config,
            commands::get_status,
            commands::shutdown_daemon
        ])
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            fs::create_dir_all(&config_dir)?;

            let config_manager: SharedConfigManager =
                Arc::new(ConfigManager::new(config_dir.join("config.json")));
            app.manage(config_manager);

            let handle = spawn_ui_bridge_task(
                app.handle().clone(),
                ui_sync_port,
                setup_shutdown.clone(),
                setup_state_sender.clone(),
            );

            if let Ok(mut guard) = setup_bridge_task.lock() {
                *guard = Some(handle);
            }

            Ok(())
        })
        .build(tauri::generate_context!())?;

    let event_shutdown = shutdown;
    let event_bridge_task = Arc::clone(&bridge_task);
    let event_is_shutting_down = Arc::clone(&is_shutting_down);

    app.run(move |app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let already_shutting_down = event_is_shutting_down.swap(true, Ordering::SeqCst);
            if already_shutting_down {
                return;
            }

            api.prevent_exit();
            shutdown_ui_bridge_and_disallow(&event_shutdown, &event_bridge_task, "shutdown");

            app_handle.exit(0);
        }
        tauri::RunEvent::Exit => {
            let already_shutting_down = event_is_shutting_down.swap(true, Ordering::SeqCst);
            if already_shutting_down {
                return;
            }

            shutdown_ui_bridge_and_disallow(&event_shutdown, &event_bridge_task, "exit");
        }
        _ => {}
    });

    Ok(())
}
