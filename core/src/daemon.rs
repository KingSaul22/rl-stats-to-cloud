use futures_util::SinkExt;
use interprocess::local_socket::{
    prelude::*, GenericNamespaced, Listener as LocalSocketListener,
    ListenerNonblockingMode, ListenerOptions, Stream as LocalSocketStream,
};
use rl_stats_core::{connector_factory, AppConfig, AppState, RocketLeagueWorker, StateReceiver};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use tokio_util::sync::CancellationToken;

const CONTROL_SOCKET_BASENAME: &str = "rl_stats_control.sock";
#[cfg(unix)]
const CONTROL_SOCKET_FALLBACK_PATH: &str = "/tmp/rl_stats_control.sock";
const UI_IDLE_AUTO_DISALLOW_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlCommand {
    AllowUi,
    DisallowUi,
    Poweroff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum ControlReply {
    Ok { message: String },
    NotRunning { message: String },
    Error { message: String },
}

struct DaemonOwnership {
    listener: LocalSocketListener,
    endpoint_display: String,
}

struct DaemonSupervisor {
    config: AppConfig,
    shutdown: CancellationToken,
}

struct UiServerTask {
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

struct UiServerControl {
    bind_addr: String,
    state_receiver: StateReceiver,
    server_task: Option<UiServerTask>,
}

pub fn execute_control_command(command: ControlCommand) {
    let reply = send_control_command(command);
    print_control_reply(command, &reply);
}

pub fn run_daemon(config: AppConfig) {
    println!("Starting rl_stats_core daemon...");

    let daemon_ownership = match claim_daemon_ownership() {
        Ok(ownership) => ownership,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let supervisor = DaemonSupervisor {
        config,
        shutdown: CancellationToken::new(),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("Failed to initialize tokio runtime: {err}");
            return;
        }
    };

    runtime.block_on(supervisor.run(daemon_ownership));
}

fn send_control_command(command: ControlCommand) -> ControlReply {
    let endpoint_display = control_endpoint_display();
    let name = match control_socket_name() {
        Ok(name) => name,
        Err(err) => {
            return ControlReply::Error {
                message: format!("Failed to resolve control endpoint name: {err}"),
            };
        }
    };

    let mut stream = match LocalSocketStream::connect(name) {
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

    if let Err(err) = stream.write_all(payload.as_bytes()) {
        return ControlReply::Error {
            message: format!("Failed to send control command payload: {err}"),
        };
    }
    if let Err(err) = stream.write_all(b"\n") {
        return ControlReply::Error {
            message: format!("Failed to send control command frame delimiter: {err}"),
        };
    }
    if let Err(err) = stream.flush() {
        return ControlReply::Error {
            message: format!("Failed to flush control command stream: {err}"),
        };
    }

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    match reader.read_line(&mut response_line) {
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
}

fn print_control_reply(command: ControlCommand, reply: &ControlReply) {
    let command_name = match command {
        ControlCommand::AllowUi => "allow-ui",
        ControlCommand::DisallowUi => "disallow-ui",
        ControlCommand::Poweroff => "poweroff",
    };

    match reply {
        ControlReply::Ok { message } => {
            println!("Command '{command_name}' acknowledged: {message}");
        }
        ControlReply::NotRunning { message } => {
            eprintln!("Command '{command_name}' failed: {message}");
        }
        ControlReply::Error { message } => {
            eprintln!("Command '{command_name}' error: {message}");
        }
    }
}

fn claim_daemon_ownership() -> Result<DaemonOwnership, String> {
    let endpoint_display = control_endpoint_display();
    let name = control_socket_name().map_err(|err| {
        format!("Failed to resolve control endpoint name {endpoint_display}: {err}")
    })?;

    ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()
        .map(|listener| DaemonOwnership {
            listener,
            endpoint_display: endpoint_display.clone(),
        })
        .map_err(|err| {
            format!(
                "Another daemon instance appears to be running (failed to bind control endpoint {}): {}",
                endpoint_display, err
            )
        })
}

impl DaemonSupervisor {
    async fn run(self, daemon_ownership: DaemonOwnership) {
        let (state_sender, state_receiver) = tokio::sync::watch::channel(AppState::default());
        let ui_control = Arc::new(AsyncMutex::new(UiServerControl {
            bind_addr: format!("127.0.0.1:{}", self.config.ui_sync_port),
            state_receiver: state_receiver.clone(),
            server_task: None,
        }));

        let shutdown = self.shutdown.clone();
        let control_ui_control = Arc::clone(&ui_control);
        let control_task = tokio::spawn(async move {
            run_control_server_loop(daemon_ownership, shutdown, control_ui_control).await;
        });

        let initial_sink = connector_factory(&self.config.connector);
        let (_sink_sender, sink_receiver) = tokio::sync::watch::channel(initial_sink);
        let worker = RocketLeagueWorker::from_config(&self.config, state_sender, sink_receiver);
        let worker_shutdown = self.shutdown.clone();
        let shutdown = self.shutdown.clone();

        let worker_task = tokio::spawn(async move {
            worker.run_until_cancelled(worker_shutdown).await;
        });

        tokio::select! {
            signal_result = tokio::signal::ctrl_c() => {
                match signal_result {
                    Ok(()) => println!("Ctrl+C received. Signaling daemon shutdown..."),
                    Err(err) => eprintln!("Failed to listen for Ctrl+C: {err}"),
                }
                shutdown.cancel();
            }
            _ = shutdown.cancelled() => {
                println!("Shutdown requested. Stopping daemon supervisor...");
            }
        }

        if let Err(err) = worker_task.await {
            eprintln!("Worker task join error: {err}");
        }

        let _ = stop_ui_server(&ui_control).await;

        if let Err(err) = control_task.await {
            eprintln!("Control server task join error: {err}");
        }
    }
}

async fn run_control_server_loop(
    ownership: DaemonOwnership,
    shutdown: CancellationToken,
    ui_control: Arc<AsyncMutex<UiServerControl>>,
) {
    println!("Control transport listening on {}", ownership.endpoint_display);

    while !shutdown.is_cancelled() {
        match ownership.listener.accept() {
            Ok(mut stream) => {
                let reply =
                    handle_control_connection(&mut stream, &shutdown, &ui_control).await;
                if let Err(err) = write_control_reply(&mut stream, &reply) {
                    eprintln!("Failed to send control reply: {err}");
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
            Err(err) => {
                eprintln!("Control listener accept error: {err}");
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        }
    }

    println!("Control server loop stopped.");
}

async fn handle_control_connection(
    stream: &mut LocalSocketStream,
    shutdown: &CancellationToken,
    ui_control: &Arc<AsyncMutex<UiServerControl>>,
) -> ControlReply {
    let mut frame = String::new();
    {
        let mut reader = BufReader::new(&mut *stream);
        match reader.read_line(&mut frame) {
            Ok(0) => {
                return ControlReply::Error {
                    message: "Received empty control payload.".to_string(),
                };
            }
            Ok(_) => {}
            Err(err) => {
                return ControlReply::Error {
                    message: format!("Failed to read control command frame: {err}"),
                };
            }
        }
    }

    let command = match serde_json::from_str::<ControlCommand>(frame.trim_end()) {
        Ok(command) => command,
        Err(err) => {
            return ControlReply::Error {
                message: format!("Invalid control command payload: {err}"),
            };
        }
    };

    dispatch_control_command(command, shutdown, ui_control).await
}

async fn dispatch_control_command(
    command: ControlCommand,
    shutdown: &CancellationToken,
    ui_control: &Arc<AsyncMutex<UiServerControl>>,
) -> ControlReply {
    match command {
        ControlCommand::AllowUi => {
            let mut guard = ui_control.lock().await;
            if guard.server_task.is_some() {
                return ControlReply::Ok {
                    message: "AllowUi acknowledged. UI server is already running.".to_string(),
                };
            }

            let bind_addr = guard.bind_addr.clone();
            let state_receiver = guard.state_receiver.clone();
            let ui_shutdown = CancellationToken::new();
            let task_shutdown = ui_shutdown.clone();
            let task_ui_control = Arc::clone(ui_control);
            let task = tokio::spawn(async move {
                run_ui_websocket_server(
                    bind_addr,
                    state_receiver,
                    task_shutdown,
                    task_ui_control,
                )
                .await;
            });
            guard.server_task = Some(UiServerTask {
                shutdown: ui_shutdown,
                task,
            });

            println!("AllowUi command received. UI websocket server started.");
            ControlReply::Ok {
                message: "AllowUi acknowledged. UI websocket server started.".to_string(),
            }
        }
        ControlCommand::DisallowUi => {
            let was_running = stop_ui_server(ui_control).await;
            if was_running {
                println!("DisallowUi command received. UI websocket server stopped.");
            } else {
                println!("DisallowUi command received. UI websocket server was not running.");
            }

            ControlReply::Ok {
                message: if was_running {
                    "DisallowUi acknowledged. UI websocket server stopped.".to_string()
                } else {
                    "DisallowUi acknowledged. UI websocket server was already stopped."
                        .to_string()
                },
            }
        }
        ControlCommand::Poweroff => {
            println!("Poweroff command received. Triggering daemon shutdown...");
            shutdown.cancel();
            let _ = stop_ui_server(ui_control).await;
            ControlReply::Ok {
                message: "Poweroff acknowledged. Daemon shutdown initiated.".to_string(),
            }
        }
    }
}

async fn stop_ui_server(ui_control: &Arc<AsyncMutex<UiServerControl>>) -> bool {
    let maybe_task = {
        let mut guard = ui_control.lock().await;
        guard.server_task.take()
    };

    if let Some(ui_task) = maybe_task {
        ui_task.shutdown.cancel();
        if let Err(err) = ui_task.task.await {
            eprintln!("UI server task join error: {err}");
        }
        true
    } else {
        false
    }
}

async fn run_ui_websocket_server(
    bind_addr: String,
    state_receiver: StateReceiver,
    shutdown: CancellationToken,
    ui_control: Arc<AsyncMutex<UiServerControl>>,
) {
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("Failed to bind UI websocket server at {bind_addr}: {err}");
            return;
        }
    };

    println!("UI websocket server listening at ws://{bind_addr}");
    let active_clients = Arc::new(AtomicUsize::new(0));
    let lifecycle_generation = Arc::new(AtomicU64::new(0));

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                println!("UI websocket server shutdown requested.");
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        let client_shutdown = shutdown.clone();
                        let client_state_receiver = state_receiver.clone();
                        let client_active_clients = Arc::clone(&active_clients);
                        let client_lifecycle_generation = Arc::clone(&lifecycle_generation);
                        let client_ui_control = Arc::clone(&ui_control);
                        tokio::spawn(async move {
                            serve_ui_client(
                                stream,
                                client_state_receiver,
                                client_shutdown,
                                client_active_clients,
                                client_lifecycle_generation,
                                client_ui_control,
                            )
                            .await;
                        });
                    }
                    Err(err) => {
                        eprintln!("UI websocket accept error: {err}");
                    }
                }
            }
        }
    }

    println!("UI websocket server stopped.");
}

async fn serve_ui_client(
    stream: TcpStream,
    state_receiver: StateReceiver,
    shutdown: CancellationToken,
    active_clients: Arc<AtomicUsize>,
    lifecycle_generation: Arc<AtomicU64>,
    ui_control: Arc<AsyncMutex<UiServerControl>>,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(err) => {
            eprintln!("Failed websocket handshake for UI client: {err}");
            return;
        }
    };

    active_clients.fetch_add(1, Ordering::SeqCst);
    let _ = lifecycle_generation.fetch_add(1, Ordering::SeqCst);

    if let Err(err) = stream_state_to_client(ws_stream, state_receiver, shutdown).await {
        eprintln!("UI websocket client stream ended with error: {err}");
    }

    let previous = active_clients.fetch_sub(1, Ordering::SeqCst);
    if previous == 1 {
        let generation_at_disconnect = lifecycle_generation.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(
                UI_IDLE_AUTO_DISALLOW_SECONDS,
            ))
            .await;

            let still_idle = active_clients.load(Ordering::SeqCst) == 0;
            let same_generation =
                lifecycle_generation.load(Ordering::SeqCst) == generation_at_disconnect;

            if still_idle && same_generation {
                println!(
                    "No UI clients reconnected within {}s. Auto-disallowing UI server.",
                    UI_IDLE_AUTO_DISALLOW_SECONDS
                );
                let _ = stop_ui_server(&ui_control).await;
            }
        });
    }
}

async fn stream_state_to_client(
    mut ws_stream: WebSocketStream<TcpStream>,
    mut state_receiver: StateReceiver,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let initial_state = state_receiver.borrow().clone();
    send_app_state(&mut ws_stream, &initial_state).await?;

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                return Ok(());
            }
            changed = state_receiver.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let next_state = state_receiver.borrow().clone();
                send_app_state(&mut ws_stream, &next_state).await?;
            }
        }
    }
}

async fn send_app_state(
    ws_stream: &mut WebSocketStream<TcpStream>,
    state: &AppState,
) -> Result<(), String> {
    let payload = serde_json::to_string(state)
        .map_err(|err| format!("Failed to serialize AppState for websocket: {err}"))?;

    ws_stream
        .send(Message::Text(payload.into()))
        .await
        .map_err(|err| format!("Failed to send websocket state update: {err}"))
}

fn write_control_reply(stream: &mut LocalSocketStream, reply: &ControlReply) -> std::io::Result<()> {
    let payload = serde_json::to_string(reply)
        .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn control_socket_name<'a>() -> std::io::Result<interprocess::local_socket::Name<'a>> {
    if GenericNamespaced::is_supported() {
        CONTROL_SOCKET_BASENAME.to_ns_name::<GenericNamespaced>()
    } else {
        #[cfg(unix)]
        {
            CONTROL_SOCKET_FALLBACK_PATH
                .to_fs_name::<interprocess::local_socket::GenericFilePath>()
        }
        #[cfg(not(unix))]
        {
            Err(std::io::Error::new(
                ErrorKind::AddrNotAvailable,
                "No supported local socket namespace available on this platform.",
            ))
        }
    }
}

fn control_endpoint_display() -> String {
    if GenericNamespaced::is_supported() {
        if cfg!(unix) {
            format!("@{CONTROL_SOCKET_BASENAME}")
        } else {
            format!("\\\\.\\pipe\\{CONTROL_SOCKET_BASENAME}")
        }
    } else {
        #[cfg(unix)]
        {
            CONTROL_SOCKET_FALLBACK_PATH.to_string()
        }
        #[cfg(not(unix))]
        {
            CONTROL_SOCKET_BASENAME.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ControlCommand;

    #[test]
    fn control_command_json_roundtrip() {
        let value = serde_json::to_string(&ControlCommand::AllowUi)
            .expect("ControlCommand should serialize");
        assert_eq!(value, "\"allow-ui\"");

        let parsed: ControlCommand = serde_json::from_str(&value)
            .expect("ControlCommand should deserialize");
        assert_eq!(parsed, ControlCommand::AllowUi);
    }
}
