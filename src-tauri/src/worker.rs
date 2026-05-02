use crate::config::AppConfig;
use crate::SinkReceiver;
use crate::StateSender;
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

type TcpTarget = (String, u16);

#[derive(Clone)]
pub struct RocketLeagueWorker {
    websocket_url: String,
    reconnect_delay: Duration,
    sink_receiver: SinkReceiver,
    state_sender: StateSender,
}

impl RocketLeagueWorker {
    #[must_use]
    pub fn from_config(
        config: &AppConfig,
        state_sender: StateSender,
        sink_receiver: SinkReceiver,
    ) -> Self {
        Self {
            websocket_url: config.websocket_url.clone(),
            reconnect_delay: Duration::from_secs(config.reconnect_delay_seconds),
            sink_receiver,
            state_sender,
        }
    }

    pub async fn run(&self) {
        self.run_until_cancelled(CancellationToken::new()).await;
    }

    pub async fn run_until_cancelled(&self, shutdown: CancellationToken) {
        let mut shutdown_logged = false;

        loop {
            if shutdown.is_cancelled() {
                self.log_shutdown_once(&mut shutdown_logged);
                break;
            }

            let session_result = tokio::select! {
                () = shutdown.cancelled() => {
                    self.log_shutdown_once(&mut shutdown_logged);
                    break;
                }
                result = self.run_session(&shutdown) => result,
            };

            match session_result {
                Ok(()) => {
                    if shutdown.is_cancelled() {
                        self.log_shutdown_once(&mut shutdown_logged);
                        break;
                    }

                    eprintln!(
                        "Session ended cleanly. Reconnecting in {} seconds...",
                        self.reconnect_delay.as_secs()
                    );
                }
                Err(err) => {
                    self.set_connected(false);
                    eprintln!(
                        "Connection/session error: {}. Retrying in {} seconds...",
                        err,
                        self.reconnect_delay.as_secs()
                    );
                }
            }

            tokio::select! {
                () = shutdown.cancelled() => {
                    self.log_shutdown_once(&mut shutdown_logged);
                    break;
                }
                () = sleep(self.reconnect_delay) => {}
            }
        }
    }

    fn log_shutdown_once(&self, shutdown_logged: &mut bool) {
        if !*shutdown_logged {
            self.set_connected(false);
            println!("Shutting down safely...");
            *shutdown_logged = true;
        }
    }

    async fn run_session(&self, shutdown: &CancellationToken) -> Result<(), String> {
        if self.websocket_url.starts_with("tcp://") {
            return self.run_raw_tcp_session(shutdown).await;
        }

        match self.run_websocket_session(shutdown).await {
            Ok(()) => Ok(()),
            Err(err) => {
                if Self::should_fallback_to_tcp(&err) {
                    eprintln!(
                        "Endpoint did not complete WebSocket handshake; falling back to raw TCP stream mode."
                    );
                    self.run_raw_tcp_session(shutdown).await
                } else {
                    Err(err)
                }
            }
        }
    }

    async fn run_websocket_session(&self, shutdown: &CancellationToken) -> Result<(), String> {
        let (stream, _) = connect_async(&self.websocket_url)
            .await
            .map_err(|err| format!("could not connect to {} ({})", self.websocket_url, err))?;

        self.set_connected(true);
        println!("Connected (WebSocket) to {}", self.websocket_url);

        let (_, mut read) = stream.split();

        loop {
            let next_message = tokio::select! {
                () = shutdown.cancelled() => {
                    self.set_connected(false);
                    return Ok(());
                }
                message = read.next() => message,
            };

            let Some(message_result) = next_message else {
                return Ok(());
            };

            let message = message_result.map_err(|err| format!("read error: {err}"))?;

            match message {
                Message::Text(text) => self.handle_payload(&text),
                Message::Binary(bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(text) => self.handle_payload(&text),
                    Err(err) => eprintln!("Skipping non-UTF8 binary frame: {err}"),
                },
                Message::Close(frame) => {
                    self.set_connected(false);
                    if let Some(frame) = frame {
                        println!(
                            "WebSocket closed by server (code: {}, reason: {}).",
                            frame.code, frame.reason
                        );
                    } else {
                        println!("WebSocket closed by server.");
                    }
                    return Ok(());
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn run_raw_tcp_session(&self, shutdown: &CancellationToken) -> Result<(), String> {
        let (host, port) = self.resolve_tcp_target()?;
        let address = format!("{host}:{port}");

        let mut stream = TcpStream::connect(&address)
            .await
            .map_err(|err| format!("could not connect to {address} ({err})"))?;

        self.set_connected(true);
        println!("Connected (TCP stream) to {address}");

        let mut read_buffer = [0_u8; 4096];
        let mut pending = String::new();

        loop {
            let read = tokio::select! {
                () = shutdown.cancelled() => {
                    self.set_connected(false);
                    return Ok(());
                }
                read_result = stream.read(&mut read_buffer) => {
                    read_result.map_err(|err| format!("tcp read error: {err}"))?
                }
            };

            if read == 0 {
                self.set_connected(false);
                return Ok(());
            }

            let chunk = String::from_utf8_lossy(&read_buffer[..read]);
            pending.push_str(&chunk);
            self.consume_json_stream(&mut pending);

            if pending.len() > 512 * 1024 {
                eprintln!("Dropping oversized undecodable TCP buffer.");
                pending.clear();
            }
        }
    }

    fn should_fallback_to_tcp(error_message: &str) -> bool {
        let lower = error_message.to_ascii_lowercase();
        lower.contains("httparse")
            || lower.contains("invalid http")
            || lower.contains("protocol error")
    }

    fn resolve_tcp_target(&self) -> Result<TcpTarget, String> {
        if let Some(raw) = self.websocket_url.strip_prefix("tcp://") {
            let mut parts = raw.rsplitn(2, ':');
            let port_str = parts
                .next()
                .ok_or_else(|| "invalid tcp URL port".to_string())?;
            let host = parts
                .next()
                .ok_or_else(|| "invalid tcp URL host".to_string())?
                .to_string();

            let port = port_str
                .parse::<u16>()
                .map_err(|err| format!("invalid tcp port: {err}"))?;

            return Ok((host, port));
        }

        let parsed = Url::parse(&self.websocket_url)
            .map_err(|err| format!("invalid websocket URL {} ({})", self.websocket_url, err))?;

        let host = parsed
            .host_str()
            .ok_or_else(|| "websocket URL missing host".to_string())?
            .to_string();

        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "websocket URL missing port".to_string())?;

        Ok((host, port))
    }

    fn consume_json_stream(&self, pending: &mut String) {
        loop {
            let mut stream = serde_json::Deserializer::from_str(pending).into_iter::<Value>();
            let item = stream.next();

            match item {
                Some(Ok(value)) => {
                    let consumed = stream.byte_offset();
                    if consumed == 0 {
                        break;
                    }
                    self.handle_value(value);
                    pending.drain(..consumed);
                }
                Some(Err(err)) => {
                    if err.is_eof() {
                        break;
                    }

                    eprintln!("Skipping malformed streamed JSON segment: {err}");
                    let consumed = stream.byte_offset().saturating_add(1);
                    if consumed > 0 && consumed <= pending.len() {
                        pending.drain(..consumed);
                    } else {
                        pending.clear();
                    }
                    break;
                }
                None => break,
            }
        }
    }

    fn handle_payload(&self, payload: &str) {
        let parsed: Value = match serde_json::from_str(payload) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Skipping invalid JSON payload: {err}");
                return;
            }
        };

        self.handle_value(parsed);
    }

    fn handle_value(&self, parsed: Value) {
        match parsed.get("Event").and_then(Value::as_str) {
            Some(event_name) => {
                self.set_last_event(event_name);
                let sink = self.sink_receiver.borrow().clone();
                let event_type = event_name.to_string();
                let payload = parsed;

                tokio::spawn(async move {
                    sink.send_event(&event_type, &payload).await;
                });
            }
            None => println!("Received JSON without Event field."),
        }
    }

    fn set_connected(&self, connected: bool) {
        let mut new_state = self.state_sender.borrow().clone();
        if new_state.is_connected != connected {
            new_state.is_connected = connected;
            let _ = self.state_sender.send(new_state);
        }
    }

    fn set_last_event(&self, event_name: &str) {
        let mut new_state = self.state_sender.borrow().clone();
        if new_state.last_event != event_name {
            new_state.last_event = event_name.to_string();
            let _ = self.state_sender.send(new_state);
        }
    }
}
