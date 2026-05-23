use super::control::stop_ui_server;
use super::{UiServerControl, UI_IDLE_AUTO_DISALLOW_SECONDS};
use crate::{AppState, StateReceiver};
use futures_util::SinkExt;
use std::sync::{
	atomic::{AtomicU64, AtomicUsize, Ordering},
	Arc,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use tokio_util::sync::CancellationToken;

pub(super) async fn run_ui_websocket_server(
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
			() = shutdown.cancelled() => {
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

pub(super) async fn serve_ui_client(
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
					"No UI clients reconnected within {UI_IDLE_AUTO_DISALLOW_SECONDS}s. Auto-disallowing UI server."
				);
				let _ = stop_ui_server(&ui_control).await;
			}
		});
	}
}

pub(super) async fn stream_state_to_client(
	mut ws_stream: WebSocketStream<TcpStream>,
	mut state_receiver: StateReceiver,
	shutdown: CancellationToken,
) -> Result<(), String> {
	let initial_state = state_receiver.borrow().clone();
	send_app_state(&mut ws_stream, &initial_state).await?;

	loop {
		tokio::select! {
			() = shutdown.cancelled() => {
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

pub(super) async fn send_app_state(
	ws_stream: &mut WebSocketStream<TcpStream>,
	state: &AppState,
) -> Result<(), String> {
	let payload = serde_json::to_string(state)
		.map_err(|err| format!("Failed to serialize AppState for websocket: {err}"))?;

	ws_stream
		.send(Message::Text(payload))
		.await
		.map_err(|err| format!("Failed to send websocket state update: {err}"))
}
