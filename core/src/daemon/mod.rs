use crate::{AppConfig, AppState, RocketLeagueWorker, StateReceiver, connector_factory};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod client;
mod control;
mod protocol;
mod ui_server;

use control::{run_control_server_loop, stop_ui_server};

pub use client::execute_control_command;
pub use protocol::ControlCommand;

const CONTROL_BIND_ADDR: &str = "127.0.0.1:43210";
const UI_IDLE_AUTO_DISALLOW_SECONDS: u64 = 30;

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

pub fn run_daemon(config: AppConfig) {
    println!("Starting rl_stats_core daemon...");

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

    runtime.block_on(supervisor.run());
}

impl DaemonSupervisor {
    async fn run(self) {
        let endpoint_display = control_endpoint_display();
        let control_listener = match TcpListener::bind(CONTROL_BIND_ADDR).await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!(
                    "Another daemon instance appears to be running (failed to bind control endpoint {endpoint_display}): {err}"
                );
                return;
            }
        };

        let (state_sender, state_receiver) = tokio::sync::watch::channel(AppState::default());
        let ui_control = Arc::new(AsyncMutex::new(UiServerControl {
            bind_addr: format!("127.0.0.1:{}", self.config.ui_sync_port),
            state_receiver: state_receiver.clone(),
            server_task: None,
        }));

        let shutdown = self.shutdown.clone();
        let control_ui_control = Arc::clone(&ui_control);
        let control_task = tokio::spawn(async move {
            run_control_server_loop(
                control_listener,
                endpoint_display,
                shutdown,
                control_ui_control,
            )
            .await;
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
            () = shutdown.cancelled() => {
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

fn control_endpoint_display() -> String {
    CONTROL_BIND_ADDR.to_string()
}

#[cfg(test)]
mod tests {
    use super::ControlCommand;
    use serde_json::Error as JsonError;

    #[test]
    fn control_command_json_roundtrip() -> Result<(), JsonError> {
        let value = serde_json::to_string(&ControlCommand::AllowUi)?;
        assert_eq!(value, "\"allow-ui\"");

        let parsed: ControlCommand = serde_json::from_str(&value)?;
        assert_eq!(parsed, ControlCommand::AllowUi);
        Ok(())
    }
}
