use super::protocol::{ControlCommand, ControlReply};
use super::ui_server::run_ui_websocket_server;
use super::{UiServerControl, UiServerTask};
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

pub(super) async fn run_control_server_loop(
	listener: TcpListener,
	endpoint_display: String,
	shutdown: CancellationToken,
	ui_control: Arc<AsyncMutex<UiServerControl>>,
) {
	println!("Control transport listening on {endpoint_display}");

	loop {
		tokio::select! {
			() = shutdown.cancelled() => {
				break;
			}
			accept_result = listener.accept() => {
				match accept_result {
					Ok((stream, _addr)) => {
						let connection_shutdown = shutdown.clone();
						let connection_ui_control = Arc::clone(&ui_control);
						tokio::spawn(async move {
							handle_control_connection(
								stream,
								connection_shutdown,
								connection_ui_control,
							)
							.await;
						});
					}
					Err(err) => {
						eprintln!("Control listener accept error: {err}");
					}
				}
			}
		}
	}

	println!("Control server loop stopped.");
}

pub(super) async fn handle_control_connection(
	stream: TcpStream,
	shutdown: CancellationToken,
	ui_control: Arc<AsyncMutex<UiServerControl>>,
) {
	let (read_half, mut write_half) = stream.into_split();
	let mut reader = TokioBufReader::new(read_half);
	let mut frame = String::new();
	let read_result = tokio::select! {
		() = shutdown.cancelled() => {
			return;
		}
		read_result = reader.read_line(&mut frame) => read_result,
	};

	let reply = match read_result {
		Ok(0) => ControlReply::Error {
			message: "Received empty control payload.".to_string(),
		},
		Ok(_) => {
			let command = match serde_json::from_str::<ControlCommand>(frame.trim_end()) {
				Ok(command) => command,
				Err(err) => {
					let reply = ControlReply::Error {
						message: format!("Invalid control command payload: {err}"),
					};
					if let Err(write_err) = write_control_reply(&mut write_half, &reply).await {
						eprintln!("Failed to send control reply: {write_err}");
					}
					return;
				}
			};

			dispatch_control_command(command, &shutdown, &ui_control).await
		}
		Err(err) => ControlReply::Error {
			message: format!("Failed to read control command frame: {err}"),
		},
	};

	if let Err(err) = write_control_reply(&mut write_half, &reply).await {
		eprintln!("Failed to send control reply: {err}");
	}
}

pub(super) async fn dispatch_control_command(
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
			drop(guard);

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

pub(super) async fn stop_ui_server(ui_control: &Arc<AsyncMutex<UiServerControl>>) -> bool {
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

pub(super) async fn write_control_reply(
	stream: &mut OwnedWriteHalf,
	reply: &ControlReply,
) -> std::io::Result<()> {
	let payload = serde_json::to_string(reply)
		.map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
	stream.write_all(payload.as_bytes()).await?;
	stream.write_all(b"\n").await?;
	stream.flush().await
}
