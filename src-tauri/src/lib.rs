pub mod commands;
pub mod config;
pub mod firebase;
pub mod worker;

pub use config::{default_config_path, AppConfig, ConfigManager};
pub use firebase::FirebaseClient;
pub use worker::RocketLeagueWorker;
use serde::Serialize;
use std::fs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;
use tauri::async_runtime::JoinHandle;
use tokio_util::sync::CancellationToken;

pub type SharedAppState = Arc<Mutex<AppState>>;
pub type SharedConfig = Arc<Mutex<AppConfig>>;
pub type SharedConfigManager = Arc<ConfigManager>;
type WorkerTask = Arc<Mutex<Option<JoinHandle<()>>>>;

#[derive(Default, Debug, Clone, Serialize)]
pub struct AppState {
    pub is_connected: bool,
    pub last_event: String,
}


// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run_tauri(config: AppConfig) -> Result<(), tauri::Error> {
    let shared_state: SharedAppState = Arc::new(Mutex::new(AppState::default()));
    let shared_config: SharedConfig = Arc::new(Mutex::new(config.clone()));
    let shutdown = CancellationToken::new();
    let is_shutting_down = Arc::new(AtomicBool::new(false));
    let worker_task: WorkerTask = Arc::new(Mutex::new(None));

    let setup_shutdown = shutdown.clone();
    let setup_config = config.clone();
    let setup_shared_state = Arc::clone(&shared_state);
    let setup_worker_task = Arc::clone(&worker_task);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::clone(&shared_state))
        .manage(Arc::clone(&shared_config))
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_config,
            commands::save_config,
            commands::get_status
        ])
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            fs::create_dir_all(&config_dir)?;

            let config_manager: SharedConfigManager = Arc::new(ConfigManager::new(
                config_dir.join("config.json"),
            ));
            app.manage(config_manager);

            let worker = RocketLeagueWorker::from_config(
                &setup_config,
                Arc::clone(&setup_shared_state),
                Some(app.handle().clone()),
            );
            let worker_shutdown = setup_shutdown.clone();
            let handle = tauri::async_runtime::spawn(async move {
                worker.run_until_cancelled(worker_shutdown).await;
            });

            if let Ok(mut guard) = setup_worker_task.lock() {
                *guard = Some(handle);
            }

            Ok(())
        })
        .build(tauri::generate_context!())?;

    let event_shutdown = shutdown.clone();
    let event_worker_task = Arc::clone(&worker_task);
    let event_is_shutting_down = Arc::clone(&is_shutting_down);

    app.run(move |app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let already_shutting_down = event_is_shutting_down.swap(true, Ordering::SeqCst);
            if already_shutting_down {
                return;
            }

            api.prevent_exit();
            event_shutdown.cancel();

            if let Ok(mut guard) = event_worker_task.lock()
                && let Some(handle) = guard.take() {
                    tauri::async_runtime::block_on(async {
                        let _ = handle.await;
                    });
                }

            app_handle.exit(0);
        }
        tauri::RunEvent::Exit => {
            event_shutdown.cancel();

            if let Ok(mut guard) = event_worker_task.lock()
                && let Some(handle) = guard.take() {
                    tauri::async_runtime::block_on(async {
                        let _ = handle.await;
                    });
                }
        }
        _ => {}
    });

    Ok(())
}
