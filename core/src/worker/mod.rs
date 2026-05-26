use crate::SinkReceiver;
use crate::StateSender;
use crate::config::AppConfig;
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

mod actors;
mod context;
mod events;
mod transformer;

use actors::{run_event_feed_actor, run_historical_actor, run_live_state_actor};
use transformer::normalize_payload;

pub use context::SessionContext;
pub use events::{IngestClass, IngestEnvelope, RocketLeagueEvent};

type TcpTarget = (String, u16);
const EVENT_FEED_CAPACITY: usize = 2_048;
const HISTORICAL_CAPACITY: usize = 8_192;
const RETRY_BACKOFF_BASE_SECONDS: u64 = 1;
const RETRY_BACKOFF_MAX_SECONDS: u64 = 32;
const EVENT_FEED_MAX_FAILURES: u32 = 3;

#[derive(Clone)]
struct RoutingLanes {
    live_state: watch::Sender<IngestEnvelope>,
    event_feed: mpsc::Sender<IngestEnvelope>,
    historical: mpsc::Sender<IngestEnvelope>,
}

#[derive(Default)]
struct RoutingStats {
    dropped_event_feed_count: u64,
}

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
        let sink = self.sink_receiver.borrow().clone();
        let mut session_context = SessionContext::new(None, None);

        let (live_state_sender, live_state_receiver) = watch::channel(IngestEnvelope::bootstrap());
        let (event_feed_sender, event_feed_receiver) = mpsc::channel(EVENT_FEED_CAPACITY);
        let (historical_sender, historical_receiver) = mpsc::channel(HISTORICAL_CAPACITY);
        let lanes = RoutingLanes {
            live_state: live_state_sender,
            event_feed: event_feed_sender,
            historical: historical_sender,
        };

        let live_task = tokio::spawn(run_live_state_actor(
            live_state_receiver,
            Arc::clone(&sink),
            shutdown.clone(),
        ));
        let event_feed_task = tokio::spawn(run_event_feed_actor(
            event_feed_receiver,
            Arc::clone(&sink),
            shutdown.clone(),
        ));
        let historical_task = tokio::spawn(run_historical_actor(
            historical_receiver,
            sink,
            shutdown.clone(),
        ));

        let mut routing_stats = RoutingStats::default();
        let mut sequence = 0_u64;

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
                result = self.run_session(
                    &shutdown,
                    &lanes,
                    &mut sequence,
                    &mut session_context,
                    &mut routing_stats,
                ) => result,
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

        shutdown.cancel();
        drop(lanes);

        Self::join_actor("live_state", live_task).await;
        Self::join_actor("event_feed", event_feed_task).await;
        Self::join_actor("historical", historical_task).await;
    }

    fn log_shutdown_once(&self, shutdown_logged: &mut bool) {
        if !*shutdown_logged {
            self.set_connected(false);
            println!("Shutting down safely...");
            *shutdown_logged = true;
        }
    }

    async fn run_session(
        &self,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
        if self.websocket_url.starts_with("tcp://") {
            return self
                .run_raw_tcp_session(shutdown, lanes, sequence, session_context, routing_stats)
                .await;
        }

        match self
            .run_websocket_session(shutdown, lanes, sequence, session_context, routing_stats)
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                if Self::should_fallback_to_tcp(&err) {
                    eprintln!(
                        "Endpoint did not complete WebSocket handshake; falling back to raw TCP stream mode."
                    );
                    self.run_raw_tcp_session(
                        shutdown,
                        lanes,
                        sequence,
                        session_context,
                        routing_stats,
                    )
                    .await
                } else {
                    Err(err)
                }
            }
        }
    }

    async fn run_websocket_session(
        &self,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
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
                Message::Text(text) => {
                    self.handle_payload(
                        &text,
                        shutdown,
                        lanes,
                        sequence,
                        session_context,
                        routing_stats,
                    )
                    .await?;
                }
                Message::Binary(bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(text) => {
                        self.handle_payload(
                            &text,
                            shutdown,
                            lanes,
                            sequence,
                            session_context,
                            routing_stats,
                        )
                        .await?;
                    }
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

    async fn run_raw_tcp_session(
        &self,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
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
            self.consume_json_stream(
                &mut pending,
                shutdown,
                lanes,
                sequence,
                session_context,
                routing_stats,
            )
            .await?;

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

    async fn consume_json_stream(
        &self,
        pending: &mut String,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
        loop {
            let mut stream = serde_json::Deserializer::from_str(pending).into_iter::<Value>();
            let item = stream.next();

            match item {
                Some(Ok(value)) => {
                    let consumed = stream.byte_offset();
                    if consumed == 0 {
                        break;
                    }

                    self.handle_value(
                        value,
                        shutdown,
                        lanes,
                        sequence,
                        session_context,
                        routing_stats,
                    )
                    .await?;
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
        Ok(())
    }

    async fn handle_payload(
        &self,
        payload: &str,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
        let parsed: Value = match serde_json::from_str(payload) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Skipping invalid JSON payload: {err}");
                return Ok(());
            }
        };

        self.handle_value(
            parsed,
            shutdown,
            lanes,
            sequence,
            session_context,
            routing_stats,
        )
        .await
    }

    async fn handle_value(
        &self,
        parsed: Value,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        sequence: &mut u64,
        session_context: &mut SessionContext,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
        let Some(event_name) = parsed.get("Event").and_then(Value::as_str) else {
            println!("Received JSON without Event field.");
            return Ok(());
        };
        let parsed_event = match serde_json::from_value::<RocketLeagueEvent>(Value::String(
            event_name.to_string(),
        )) {
            Ok(event) => event,
            Err(err) => {
                eprintln!("Failed to parse Event field '{event_name}': {err}");
                RocketLeagueEvent::Unknown(event_name.to_string())
            }
        };

        if event_name.is_empty() {
            println!("Received JSON with empty Event field.");
            Ok(())
        } else {
            session_context.update_from_payload(&parsed);
            self.set_last_event(event_name);
            *sequence = sequence.saturating_add(1);
            let class = Self::classify_event(&parsed_event);
            let payload = normalize_payload(class, &parsed, event_name, session_context);
            let envelope = IngestEnvelope {
                seq: *sequence,
                event_type: event_name.to_string(),
                payload,
                class,
                active_match_id: session_context.active_match_id.clone(),
            };

            self.route_envelope(envelope, shutdown, lanes, routing_stats)
                .await
        }
    }

    async fn route_envelope(
        &self,
        envelope: IngestEnvelope,
        shutdown: &CancellationToken,
        lanes: &RoutingLanes,
        routing_stats: &mut RoutingStats,
    ) -> Result<(), String> {
        match envelope.class {
            IngestClass::LiveState => {
                let _ = lanes.live_state.send_replace(envelope);
                Ok(())
            }
            IngestClass::EventFeed => match lanes.event_feed.try_send(envelope) {
                Ok(()) => Ok(()),
                Err(mpsc::error::TrySendError::Full(dropped)) => {
                    routing_stats.dropped_event_feed_count =
                        routing_stats.dropped_event_feed_count.saturating_add(1);
                    eprintln!(
                        "Dropping event_feed payload due to saturation (dropped_total={}, seq={}, event={}).",
                        routing_stats.dropped_event_feed_count, dropped.seq, dropped.event_type
                    );
                    Ok(())
                }
                Err(mpsc::error::TrySendError::Closed(dropped)) => Err(format!(
                    "event_feed actor stopped before routing seq={} event={}.",
                    dropped.seq, dropped.event_type
                )),
            },
            IngestClass::Historical => {
                tokio::select! {
                    () = shutdown.cancelled() => {
                        Err("Shutdown requested before historical payload routing completed.".to_string())
                    }
                    send_result = lanes.historical.send(envelope) => {
                        send_result.map_err(|err| format!(
                            "historical actor stopped before routing seq={} event={}.",
                            err.0.seq,
                            err.0.event_type
                        ))
                    }
                }
            }
        }
    }

    const fn classify_event(event: &RocketLeagueEvent) -> IngestClass {
        match event {
            RocketLeagueEvent::UpdateState | RocketLeagueEvent::ClockUpdated => {
                IngestClass::LiveState
            }
            RocketLeagueEvent::EventFeedMarker | RocketLeagueEvent::MatchHistoryMarker => {
                IngestClass::EventFeed
            }
            RocketLeagueEvent::Goal
            | RocketLeagueEvent::Save
            | RocketLeagueEvent::Demolition
            | RocketLeagueEvent::Unknown(_) => IngestClass::Historical,
        }
    }

    async fn join_actor(actor_name: &str, task: JoinHandle<()>) {
        if let Err(err) = task.await {
            eprintln!("Telemetry {actor_name} actor join error: {err}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn taxonomy_maps_clock_variants_to_live_state() {
        let updates = [
            RocketLeagueEvent::from_event_name("UpdateState".to_string()),
            RocketLeagueEvent::from_event_name("ClockUpdated".to_string()),
            RocketLeagueEvent::from_event_name("ClockUpdatedSeconds".to_string()),
        ];

        for event in updates {
            assert!(matches!(
                RocketLeagueWorker::classify_event(&event),
                IngestClass::LiveState
            ));
        }
    }

    #[test]
    fn ingress_normalization_produces_v2_live_state_shape() {
        let session_context = SessionContext::new(
            Some("match_cfg_1".to_string()),
            Some("session_cfg_1".to_string()),
        );
        let raw = json!({
            "Event": "UpdateState",
            "time_remaining_seconds": 123,
            "blue": 2,
            "orange": 1,
            "player_state": {
                "player_id": "player_1_id",
                "boost": 85,
                "score": 450,
                "goals": 1
            }
        });

        let normalized = normalize_payload(
            IngestClass::LiveState,
            &raw,
            "UpdateState",
            &session_context,
        );

        assert_eq!(normalized.get("is_active"), Some(&Value::Bool(true)));
        assert_eq!(
            normalized.get("session_id"),
            Some(&Value::String("session_cfg_1".to_string()))
        );
        assert_eq!(
            normalized.get("match_id"),
            Some(&Value::String("match_cfg_1".to_string()))
        );
        assert_eq!(
            normalized.get("time_remaining_seconds"),
            Some(&Value::from(123_u64))
        );

        assert!(normalized.get("score").and_then(Value::as_object).is_some());
        let Some(score) = normalized.get("score").and_then(Value::as_object) else {
            return;
        };
        assert_eq!(score.get("blue"), Some(&Value::from(2_u64)));
        assert_eq!(score.get("orange"), Some(&Value::from(1_u64)));

        assert!(
            normalized
                .get("player_telemetry")
                .and_then(Value::as_object)
                .is_some()
        );
        let Some(players) = normalized
            .get("player_telemetry")
            .and_then(Value::as_object)
        else {
            return;
        };
        assert!(
            players
                .get("player_1_id")
                .and_then(Value::as_object)
                .is_some()
        );
        let Some(player) = players.get("player_1_id").and_then(Value::as_object) else {
            return;
        };
        assert_eq!(player.get("boost"), Some(&Value::from(85_u64)));
        assert_eq!(player.get("score"), Some(&Value::from(450_i64)));
        assert_eq!(player.get("goals"), Some(&Value::from(1_u64)));
    }

    #[test]
    fn fallback_match_identity_is_generated_and_prefixed() {
        let mut session_context = SessionContext::new(None, None);
        let raw = json!({
            "Event": "Goal",
            "timestamp_ms": 1_715_000_120_000_u64,
            "game_seconds_remaining": 280_u64,
            "details": {"speed_kph": 105_u64}
        });

        session_context.update_from_payload(&raw);

        assert!(session_context.active_match_id.starts_with("match_"));
        let suffix = session_context.active_match_id.strip_prefix("match_");
        assert!(
            matches!(suffix, Some(value) if !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
        );

        let normalized = normalize_payload(IngestClass::Historical, &raw, "Goal", &session_context);
        assert_eq!(
            normalized.get("match_id"),
            Some(&Value::String(session_context.active_match_id.clone()))
        );
        assert_eq!(
            normalized.get("session_id"),
            Some(&Value::String(session_context.active_session_id.clone()))
        );
    }

    #[test]
    fn classify_event_segments_lanes_cleanly() {
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::UpdateState),
            IngestClass::LiveState
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::ClockUpdated),
            IngestClass::LiveState
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::EventFeedMarker),
            IngestClass::EventFeed
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::MatchHistoryMarker),
            IngestClass::EventFeed
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::Goal),
            IngestClass::Historical
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::Save),
            IngestClass::Historical
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::Demolition),
            IngestClass::Historical
        ));
        assert!(matches!(
            RocketLeagueWorker::classify_event(&RocketLeagueEvent::Unknown("Other".to_string())),
            IngestClass::Historical
        ));
    }
}
