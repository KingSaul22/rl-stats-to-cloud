pub mod commands;
use futures_util::StreamExt;
use rl_stats_core::{AppConfig, AppState, ConfigManager, StateReceiver, StateSender};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager};
use tauri::async_runtime::JoinHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

pub type SharedConfig = Arc<Mutex<AppConfig>>;
pub type SharedConfigManager = Arc<ConfigManager>;
pub type BridgeTaskHandle = JoinHandle<()>;
pub type SharedBridgeTask = Arc<Mutex<Option<BridgeTaskHandle>>>;

const CONTROL_ENDPOINT: &str = "127.0.0.1:43210";
const CONTROL_IO_TIMEOUT_SECONDS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ControlCommand {
    AllowUi,
    DisallowUi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum ControlReply {
    Ok { message: String },
    NotRunning { message: String },
    Error { message: String },
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

/// Run the Tauri application with the provided configuration.
/// This function initializes the application state, sets up the worker task, and handles graceful shutdown on exit events.
/// 
/// # Errors
/// Returns a `tauri::Error` if the application fails to build or run.
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
            commands::get_status
        ])
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            fs::create_dir_all(&config_dir)?;

            let config_manager: SharedConfigManager = Arc::new(ConfigManager::new(
                config_dir.join("config.json"),
            ));
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
            shutdown_ui_bridge_and_disallow(
                &event_shutdown,
                &event_bridge_task,
                "shutdown",
            );

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

fn spawn_ui_bridge_task(
    app_handle: tauri::AppHandle,
    ui_sync_port: u16,
    shutdown: CancellationToken,
    state_sender: StateSender,
) -> BridgeTaskHandle {
    let ws_url = format!("ws://127.0.0.1:{ui_sync_port}");
    tauri::async_runtime::spawn(async move {
        let allow_reply = send_control_command(ControlCommand::AllowUi).await;
        match allow_reply {
            ControlReply::Ok { message } => {
                println!("AllowUi command acknowledged from UI bridge: {message}");
            }
            ControlReply::NotRunning { message } => {
                eprintln!("AllowUi failed (daemon not running): {message}");
            }
            ControlReply::Error { message } => {
                eprintln!("AllowUi command error: {message}");
            }
        }

        run_state_bridge_loop(ws_url, app_handle, state_sender, shutdown).await;
    })
}

fn shutdown_ui_bridge_and_disallow(
    shutdown: &CancellationToken,
    bridge_task: &SharedBridgeTask,
    phase: &str,
) {
    shutdown.cancel();

    if let Ok(mut guard) = bridge_task.lock()
        && let Some(handle) = guard.take()
    {
        tauri::async_runtime::block_on(async {
            let _ = handle.await;
        });
    }

    let disallow_reply = tauri::async_runtime::block_on(async {
        send_control_command(ControlCommand::DisallowUi).await
    });
    match disallow_reply {
        ControlReply::Ok { message } => {
            println!("DisallowUi command acknowledged on UI {phase}: {message}");
        }
        ControlReply::NotRunning { message } => {
            eprintln!("DisallowUi on {phase}: daemon not running: {message}");
        }
        ControlReply::Error { message } => {
            eprintln!("DisallowUi on {phase} error: {message}");
        }
    }
}

async fn run_state_bridge_loop(
    ws_url: String,
    app_handle: tauri::AppHandle,
    state_sender: StateSender,
    shutdown: CancellationToken,
) {
    while !shutdown.is_cancelled() {
        match connect_async(&ws_url).await {
            Ok((mut ws_stream, _response)) => {
                println!("Connected to daemon UI stream at {ws_url}");

                loop {
                    tokio::select! {
                        () = shutdown.cancelled() => {
                            return;
                        }
                        next_message = ws_stream.next() => {
                            match next_message {
                                Some(Ok(Message::Text(text))) => {
                                    handle_state_message(text.as_ref(), &state_sender, &app_handle);
                                }
                                Some(Ok(Message::Binary(binary))) => {
                                    match std::str::from_utf8(&binary) {
                                        Ok(text) => {
                                            handle_state_message(text, &state_sender, &app_handle);
                                        }
                                        Err(err) => {
                                            eprintln!("Received non-UTF8 binary state payload: {err}");
                                        }
                                    }
                                }
                                Some(Ok(Message::Close(_))) => {
                                    eprintln!("Daemon UI websocket connection closed.");
                                    break;
                                }
                                Some(Ok(_)) => {}
                                Some(Err(err)) => {
                                    eprintln!("Daemon UI websocket read error: {err}");
                                    break;
                                }
                                None => {
                                    eprintln!("Daemon UI websocket stream ended.");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("Failed to connect to daemon UI stream at {ws_url}: {err}");
            }
        }

        if shutdown.is_cancelled() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

fn handle_state_message(payload: &str, state_sender: &StateSender, app_handle: &tauri::AppHandle) {
    match serde_json::from_str::<AppState>(payload) {
        Ok(state) => {
            let _ = state_sender.send(state.clone());
            if let Err(err) = app_handle.emit("status-update", state) {
                eprintln!("failed to emit status-update event: {err}");
            }
        }
        Err(err) => {
            eprintln!("Failed to decode daemon AppState payload '{payload}': {err}");
        }
    }
}

async fn send_control_command(command: ControlCommand) -> ControlReply {
    let endpoint_display = control_endpoint_display();
    let operation = async {
        let mut stream = match TcpStream::connect(CONTROL_ENDPOINT).await {
            Ok(stream) => stream,
            Err(err) => {
                return ControlReply::NotRunning {
                    message: format!("Failed to connect to daemon at {endpoint_display}: {err}"),
                };
            }
        };

        let payload = match serde_json::to_string(&command) {
            Ok(payload) => payload,
            Err(err) => {
                return ControlReply::Error {
                    message: format!("Failed to serialize control command: {err}"),
                };
            }
        };

        if let Err(err) = stream.write_all(payload.as_bytes()).await {
            return ControlReply::Error {
                message: format!("Failed to send control command payload: {err}"),
            };
        }
        if let Err(err) = stream.write_all(b"\n").await {
            return ControlReply::Error {
                message: format!("Failed to send control command frame delimiter: {err}"),
            };
        }
        if let Err(err) = stream.flush().await {
            return ControlReply::Error {
                message: format!("Failed to flush control command stream: {err}"),
            };
        }

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        match reader.read_line(&mut response_line).await {
            Ok(0) => ControlReply::Error {
                message: "Daemon closed the control socket without sending a reply.".to_string(),
            },
            Ok(_) => {
                let frame = response_line.trim_end();
                match serde_json::from_str::<ControlReply>(frame) {
                    Ok(reply) => reply,
                    Err(err) => ControlReply::Error {
                        message: format!("Failed to decode daemon reply '{frame}': {err}"),
                    },
                }
            }
            Err(err) => ControlReply::Error {
                message: format!("Failed to read control reply from daemon: {err}"),
            },
        }
    };

    timeout(
        tokio::time::Duration::from_secs(CONTROL_IO_TIMEOUT_SECONDS),
        operation,
    )
    .await
    .unwrap_or_else(|_| ControlReply::Error {
            message: format!(
                "Timed out waiting for daemon control reply after {CONTROL_IO_TIMEOUT_SECONDS}s."
            ),
        })
}

fn control_endpoint_display() -> String {
    CONTROL_ENDPOINT.to_string()
}
