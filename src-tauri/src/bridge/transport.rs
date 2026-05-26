use futures_util::StreamExt;
use rl_stats_core::{AppState, StateSender};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

pub(super) const CONTROL_ENDPOINT: &str = "127.0.0.1:43210";
pub(super) const CONTROL_IO_TIMEOUT_SECONDS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ControlCommand {
    AllowUi,
    DisallowUi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(super) enum ControlReply {
    Ok { message: String },
    NotRunning { message: String },
    Error { message: String },
}

pub(super) async fn send_control_command(command: ControlCommand) -> ControlReply {
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

pub(super) async fn run_state_bridge_loop(
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

pub(super) fn handle_state_message(
    payload: &str,
    state_sender: &StateSender,
    app_handle: &tauri::AppHandle,
) {
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

pub(super) fn control_endpoint_display() -> String {
    CONTROL_ENDPOINT.to_string()
}
