use crate::config::AppConfig;
use crate::connector::{SinkError, TelemetrySink};
use crate::SinkReceiver;
use crate::StateSender;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

type TcpTarget = (String, u16);
const EVENT_FEED_CAPACITY: usize = 2_048;
const HISTORICAL_CAPACITY: usize = 8_192;
const RETRY_BACKOFF_BASE_SECONDS: u64 = 1;
const RETRY_BACKOFF_MAX_SECONDS: u64 = 32;
const EVENT_FEED_MAX_FAILURES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IngestClass {
    LiveState,
    EventFeed,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RocketLeagueEvent {
    UpdateState,
    ClockUpdated,
    Goal,
    Save,
    Demolition,
    EventFeedMarker,
    MatchHistoryMarker,
    Unknown(String),
}

impl RocketLeagueEvent {
    #[must_use]
    fn from_event_name(event_name: String) -> Self {
        match event_name.as_str() {
            "UpdateState" => Self::UpdateState,
            "ClockUpdated" | "ClockUpdatedSeconds" => Self::ClockUpdated,
            "Goal" | "GoalScored" => Self::Goal,
            "Save" | "EpicSave" => Self::Save,
            "Demolition" | "Demo" => Self::Demolition,
            "EventFeedMarker" => Self::EventFeedMarker,
            "MatchHistoryMarker" => Self::MatchHistoryMarker,
            _ => Self::Unknown(event_name),
        }
    }
}

impl<'de> Deserialize<'de> for RocketLeagueEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let event_name = String::deserialize(deserializer)?;
        Ok(Self::from_event_name(event_name))
    }
}

#[derive(Debug, Clone)]
struct IngestEnvelope {
    seq: u64,
    event_type: String,
    payload: Value,
    class: IngestClass,
    active_match_id: String,
}

impl IngestEnvelope {
    #[must_use]
    fn bootstrap() -> Self {
        Self {
            seq: 0,
            event_type: "__bootstrap__".to_string(),
            payload: Value::Null,
            class: IngestClass::LiveState,
            active_match_id: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct SessionContext {
    active_match_id: String,
    active_session_id: String,
    id_source: String,
}

impl SessionContext {
    #[must_use]
    fn new(config_match_id: Option<String>, config_session_id: Option<String>) -> Self {
        let mut context = Self {
            active_match_id: String::new(),
            active_session_id: String::new(),
            id_source: "Generated".to_string(),
        };

        if let Some(match_id) = config_match_id
            && !match_id.is_empty()
        {
            context.active_match_id = match_id;
            context.id_source = "Config".to_string();
        }

        if let Some(session_id) = config_session_id
            && !session_id.is_empty()
        {
            context.active_session_id = session_id;
            context.id_source = "Config".to_string();
        }

        context.ensure_fallback_ids();
        context
    }

    fn update_from_payload(&mut self, payload: &Value) {
        const MATCH_KEYS: &[&str] = &[
            "match_id",
            "matchId",
            "MatchID",
            "MatchId",
            "game_id",
            "gameId",
            "GameID",
        ];
        const SESSION_KEYS: &[&str] = &[
            "session_id",
            "sessionId",
            "SessionID",
            "SessionId",
            "game_session_id",
            "gameSessionId",
            "GameSessionID",
        ];

        let telemetry_match_id = Self::extract_identifier(payload, MATCH_KEYS);
        let telemetry_session_id = Self::extract_identifier(payload, SESSION_KEYS);

        if let Some(match_id) = telemetry_match_id
            && !match_id.is_empty()
        {
            self.active_match_id = match_id;
            self.id_source = "Telemetry".to_string();
        }

        if let Some(session_id) = telemetry_session_id
            && !session_id.is_empty()
        {
            self.active_session_id = session_id;
            self.id_source = "Telemetry".to_string();
        }

        self.ensure_fallback_ids();
    }

    fn ensure_fallback_ids(&mut self) {
        if self.active_match_id.is_empty() {
            self.active_match_id = Self::generate_fallback_id("match");
            if self.id_source != "Config" {
                self.id_source = "Generated".to_string();
            }
        }

        if self.active_session_id.is_empty() {
            self.active_session_id = Self::generate_fallback_id("session");
            if self.id_source != "Config" {
                self.id_source = "Generated".to_string();
            }
        }
    }

    #[must_use]
    fn extract_identifier(payload: &Value, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| payload.get(key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    #[must_use]
    fn generate_fallback_id(prefix: &str) -> String {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0_u128, |duration| duration.as_millis());
        format!("{prefix}_{timestamp_ms}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizedPayloadKind {
    LiveState,
    EventFeed,
    Historical,
}

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
                .run_raw_tcp_session(
                    shutdown,
                    lanes,
                    sequence,
                    session_context,
                    routing_stats,
                )
                .await;
        }

        match self
            .run_websocket_session(
                shutdown,
                lanes,
                sequence,
                session_context,
                routing_stats,
            )
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
            let payload = Self::normalize_payload(
                class,
                &parsed,
                event_name,
                session_context,
            );
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

    fn normalize_payload(
        class: IngestClass,
        raw: &Value,
        event_type: &str,
        session_context: &SessionContext,
    ) -> Value {
        match Self::normalized_payload_kind(class) {
            NormalizedPayloadKind::LiveState => Self::normalize_live_state(raw, session_context),
            NormalizedPayloadKind::EventFeed => {
                Self::normalize_event_feed(raw, event_type, session_context)
            }
            NormalizedPayloadKind::Historical => {
                Self::normalize_historical(raw, event_type, session_context)
            }
        }
    }

    const fn normalized_payload_kind(class: IngestClass) -> NormalizedPayloadKind {
        match class {
            IngestClass::LiveState => NormalizedPayloadKind::LiveState,
            IngestClass::EventFeed => NormalizedPayloadKind::EventFeed,
            IngestClass::Historical => NormalizedPayloadKind::Historical,
        }
    }

    fn normalize_live_state(raw: &Value, session_context: &SessionContext) -> Value {
        let time_remaining_seconds = Self::extract_u64_from_keys(
            raw,
            &[
                "time_remaining_seconds",
                "timeRemainingSeconds",
                "seconds_remaining",
                "secondsRemaining",
                "remaining_seconds",
                "remainingSeconds",
                "clock_seconds_remaining",
                "clockSecondsRemaining",
            ],
        )
        .unwrap_or(0);

        let score = Self::extract_score_object(raw);
        let player_telemetry = Self::extract_player_telemetry(raw);

        let mut payload = Map::new();
        payload.insert("is_active".to_string(), Value::Bool(true));
        payload.insert(
            "session_id".to_string(),
            Value::String(session_context.active_session_id.clone()),
        );
        payload.insert(
            "match_id".to_string(),
            Value::String(session_context.active_match_id.clone()),
        );
        payload.insert(
            "time_remaining_seconds".to_string(),
            Value::from(time_remaining_seconds),
        );
        payload.insert("score".to_string(), score);
        payload.insert("player_telemetry".to_string(), player_telemetry);
        Value::Object(payload)
    }

    fn normalize_event_feed(
        raw: &Value,
        event_type: &str,
        session_context: &SessionContext,
    ) -> Value {
        let mut payload = Map::new();
        payload.insert(
            "timestamp_ms".to_string(),
            Value::from(Self::current_timestamp_ms()),
        );
        payload.insert(
            "game_seconds_remaining".to_string(),
            Value::from(Self::extract_game_seconds_remaining(raw).unwrap_or(0)),
        );
        payload.insert(
            "type".to_string(),
            Value::String(Self::canonical_event_type(event_type)),
        );
        payload.insert(
            "match_id".to_string(),
            Value::String(session_context.active_match_id.clone()),
        );
        payload.insert(
            "session_id".to_string(),
            Value::String(session_context.active_session_id.clone()),
        );

        if let Some(attacker_id) = Self::extract_string_from_keys(
            raw,
            &[
                "attacker_id",
                "attackerId",
                "player_id",
                "playerId",
                "scorer_id",
                "scorerId",
                "actor_id",
                "actorId",
            ],
        ) {
            payload.insert("attacker_id".to_string(), Value::String(attacker_id));
        }

        if let Some(victim_id) = Self::extract_string_from_keys(
            raw,
            &[
                "victim_id",
                "victimId",
                "target_id",
                "targetId",
                "defender_id",
                "defenderId",
            ],
        ) {
            payload.insert("victim_id".to_string(), Value::String(victim_id));
        }

        Value::Object(payload)
    }

    fn normalize_historical(
        raw: &Value,
        event_type: &str,
        session_context: &SessionContext,
    ) -> Value {
        let mut payload = Map::new();
        payload.insert(
            "timestamp_ms".to_string(),
            Value::from(Self::extract_timestamp_ms(raw).unwrap_or_else(Self::current_timestamp_ms)),
        );
        payload.insert(
            "game_seconds_remaining".to_string(),
            Value::from(Self::extract_game_seconds_remaining(raw).unwrap_or(0)),
        );
        payload.insert(
            "type".to_string(),
            Value::String(Self::canonical_event_type(event_type)),
        );
        payload.insert(
            "match_id".to_string(),
            Value::String(session_context.active_match_id.clone()),
        );
        payload.insert(
            "session_id".to_string(),
            Value::String(session_context.active_session_id.clone()),
        );

        if let Some(player_id) = Self::extract_string_from_keys(
            raw,
            &[
                "player_id",
                "playerId",
                "attacker_id",
                "attackerId",
                "scorer_id",
                "scorerId",
                "actor_id",
                "actorId",
            ],
        ) {
            payload.insert("player_id".to_string(), Value::String(player_id));
        }

        let details = Self::extract_details_object(raw);
        payload.insert("details".to_string(), details);
        Value::Object(payload)
    }

    fn canonical_event_type(event_type: &str) -> String {
        match event_type {
            "UpdateState" | "ClockUpdated" | "ClockUpdatedSeconds" => {
                "live_state".to_string()
            }
            "Goal" | "GoalScored" => "goal".to_string(),
            "Save" | "EpicSave" => "save".to_string(),
            "Demolition" | "Demo" => "demo".to_string(),
            other => other.to_ascii_lowercase(),
        }
    }

    fn current_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0_u128, |duration| duration.as_millis())
            .try_into()
            .map_or(u64::MAX, |value| value)
    }

    fn extract_timestamp_ms(raw: &Value) -> Option<u64> {
        Self::extract_u64_from_keys(
            raw,
            &[
                "timestamp_ms",
                "timestampMs",
                "TimestampMs",
                "timestamp",
                "Timestamp",
            ],
        )
    }

    fn extract_game_seconds_remaining(raw: &Value) -> Option<u64> {
        Self::extract_u64_from_keys(
            raw,
            &[
                "game_seconds_remaining",
                "gameSecondsRemaining",
                "seconds_remaining",
                "secondsRemaining",
                "time_remaining_seconds",
                "timeRemainingSeconds",
                "remaining_seconds",
                "remainingSeconds",
            ],
        )
    }

    fn extract_score_object(raw: &Value) -> Value {
        let blue = Self::extract_u64_from_keys(
            raw,
            &[
                "blue",
                "blue_score",
                "blueScore",
                "score_blue",
                "scoreBlue",
            ],
        )
        .unwrap_or(0);
        let orange = Self::extract_u64_from_keys(
            raw,
            &[
                "orange",
                "orange_score",
                "orangeScore",
                "score_orange",
                "scoreOrange",
            ],
        )
        .unwrap_or(0);

        let mut score = Map::new();
        score.insert("blue".to_string(), Value::from(blue));
        score.insert("orange".to_string(), Value::from(orange));
        Value::Object(score)
    }

    fn extract_player_telemetry(raw: &Value) -> Value {
        let mut players = Map::new();
        Self::collect_player_telemetry(raw, &mut players, None);
        Value::Object(players)
    }

    fn collect_player_telemetry(
        raw: &Value,
        players: &mut Map<String, Value>,
        parent_key: Option<&str>,
    ) {
        match raw {
            Value::Object(object) => {
                let player_id = Self::extract_string_from_keys(
                    raw,
                    &[
                        "player_id",
                        "playerId",
                        "id",
                        "Id",
                        "unique_id",
                        "uniqueId",
                        "steam_id",
                        "steamId",
                        "epic_id",
                        "epicId",
                    ],
                )
                .or_else(|| parent_key.map(ToString::to_string));

                let mut telemetry = Map::new();

                if let Some(boost) = Self::extract_u64_from_keys(raw, &["boost", "Boost"]) {
                    telemetry.insert("boost".to_string(), Value::from(boost));
                }
                if let Some(score) = Self::extract_i64_from_keys(raw, &["score", "Score"]) {
                    telemetry.insert("score".to_string(), Value::from(score));
                }
                if let Some(goals) = Self::extract_u64_from_keys(raw, &["goals", "Goals"]) {
                    telemetry.insert("goals".to_string(), Value::from(goals));
                }

                if !telemetry.is_empty()
                    && let Some(player_id) = player_id
                {
                    players.insert(player_id, Value::Object(telemetry));
                }

                for (key, value) in object {
                    Self::collect_player_telemetry(value, players, Some(key));
                }
            }
            Value::Array(values) => {
                for value in values {
                    Self::collect_player_telemetry(value, players, parent_key);
                }
            }
            _ => {}
        }
    }

    fn extract_details_object(raw: &Value) -> Value {
        if let Some(details) = raw.get("details")
            && details.is_object()
        {
            return details.clone();
        }

        let mut details = Map::new();

        if let Some(speed_kph) = Self::extract_u64_from_keys(raw, &["speed_kph", "speedKph"]) {
            details.insert("speed_kph".to_string(), Value::from(speed_kph));
        }

        Value::Object(details)
    }

    fn extract_string_from_keys(raw: &Value, keys: &[&str]) -> Option<String> {
        Self::find_value_by_keys(raw, keys)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn extract_u64_from_keys(raw: &Value, keys: &[&str]) -> Option<u64> {
        Self::find_value_by_keys(raw, keys).and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
                .or_else(|| value.as_str().and_then(|text| text.trim().parse::<u64>().ok()))
        })
    }

    fn extract_i64_from_keys(raw: &Value, keys: &[&str]) -> Option<i64> {
        Self::find_value_by_keys(raw, keys).and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .or_else(|| value.as_str().and_then(|text| text.trim().parse::<i64>().ok()))
        })
    }

    fn find_value_by_keys<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Value> {
        match raw {
            Value::Object(object) => {
                for key in keys {
                    if let Some(value) = object.get(*key) {
                        return Some(value);
                    }
                }

                object
                    .values()
                    .find_map(|value| Self::find_value_by_keys(value, keys))
            }
            Value::Array(values) => values
                .iter()
                .find_map(|value| Self::find_value_by_keys(value, keys)),
            _ => None,
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

async fn run_live_state_actor(
    mut receiver: watch::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    let mut last_sent_seq = 0_u64;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            changed = receiver.changed() => {
                if changed.is_err() {
                    break;
                }

                let envelope = receiver.borrow_and_update().clone();
                if envelope.seq <= last_sent_seq {
                    continue;
                }
                last_sent_seq = envelope.seq;

                if let Err(err) = sink.send_event(&envelope.event_type, &envelope.payload).await {
                    log_sink_failure("live_state", &envelope, &err);
                }
            }
        }
    }
}

async fn run_event_feed_actor(
    mut receiver: mpsc::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_envelope = receiver.recv() => {
                let Some(envelope) = maybe_envelope else {
                    break;
                };

                send_with_retry_policy(
                    "event_feed",
                    &envelope,
                    &sink,
                    &shutdown,
                    Some(EVENT_FEED_MAX_FAILURES),
                ).await;
            }
        }
    }
}

async fn run_historical_actor(
    mut receiver: mpsc::Receiver<IngestEnvelope>,
    sink: Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            maybe_envelope = receiver.recv() => {
                let Some(envelope) = maybe_envelope else {
                    break;
                };

                send_with_retry_policy("historical", &envelope, &sink, &shutdown, None).await;
            }
        }
    }
}

async fn send_with_retry_policy(
    lane: &str,
    envelope: &IngestEnvelope,
    sink: &Arc<dyn TelemetrySink + Send + Sync>,
    shutdown: &CancellationToken,
    max_failures: Option<u32>,
) {
    let mut consecutive_failures = 0_u32;

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        match sink.send_event(&envelope.event_type, &envelope.payload).await {
            Ok(()) => return,
            Err(SinkError::Terminal { message }) => {
                eprintln!(
                    "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                    envelope.seq, envelope.event_type, envelope.active_match_id, message
                );
                return;
            }
            Err(SinkError::RateLimited { message } | SinkError::TransientNetwork { message }) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if let Some(limit) = max_failures
                    && consecutive_failures > limit
                {
                    eprintln!(
                        "Sink retry budget exceeded [{lane}] seq={} event={} match_id={} failures={} dropped payload: {}",
                        envelope.seq,
                        envelope.event_type,
                        envelope.active_match_id,
                        consecutive_failures,
                        message
                    );
                    return;
                }

                let backoff_delay = calculate_full_jitter_backoff(consecutive_failures);
                eprintln!(
                    "Sink transient failure [{lane}] seq={} event={} match_id={} failures={} retrying_in_ms={} error={}",
                    envelope.seq,
                    envelope.event_type,
                    envelope.active_match_id,
                    consecutive_failures,
                    backoff_delay.as_millis(),
                    message
                );

                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = sleep(backoff_delay) => {}
                }
            }
        }
    }
}

fn calculate_full_jitter_backoff(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(5);
    let max_seconds = RETRY_BACKOFF_BASE_SECONDS
        .saturating_mul(1_u64 << exponent)
        .min(RETRY_BACKOFF_MAX_SECONDS);
    let max_window = Duration::from_secs(max_seconds);

    let max_millis = duration_millis_u64(max_window);
    let jitter_millis = sample_uniform_jitter_millis(max_millis);
    Duration::from_millis(jitter_millis)
}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).map_or(u64::MAX, |value| value)
}

fn sample_uniform_jitter_millis(max_millis_inclusive: u64) -> u64 {
    if max_millis_inclusive == 0 {
        return 0;
    }

    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_nanos());
    let modulus = u128::from(max_millis_inclusive).saturating_add(1);
    let sampled = epoch_nanos % modulus;

    u64::try_from(sampled).map_or(max_millis_inclusive, |value| value)
}

fn log_sink_failure(lane: &str, envelope: &IngestEnvelope, error: &SinkError) {
    match error {
        SinkError::RateLimited { message } | SinkError::TransientNetwork { message } => {
            eprintln!(
                "Sink warning [{lane}] seq={} event={} match_id={} error={} (backoff TODO).",
                envelope.seq, envelope.event_type, envelope.active_match_id, message
            );
        }
        SinkError::Terminal { message } => {
            eprintln!(
                "Sink terminal error [{lane}] seq={} event={} match_id={} dropped payload: {}",
                envelope.seq, envelope.event_type, envelope.active_match_id, message
            );
        }
    }
}
