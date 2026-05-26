# Data Pipeline

The ingestion pipeline is a five-stage, multi-lane architecture designed to prevent slow or failing
sinks from blocking time-sensitive telemetry.

## Stage 1: Ingestion

The Ingestion Engine opens a persistent connection to `websocketUrl` (default `ws://127.0.0.1:49123`).
Two transport modes are supported:

| Mode | Detection | Parser |
|------|-----------|--------|
| WebSocket | Default | `tokio-tungstenite` |
| TCP (raw) | Fallback on HTTP parse error | Line-delimited JSON stream (512 KB bounded buffer) |

On connection loss, the engine sleeps for `reconnectDelaySeconds` (default 5) and retries
indefinitely. Connection state changes are broadcast to the `state_sender` watch channel,
which feeds both the UI Sync Server and the LiveState sink.

## Stage 2: Classification

Each parsed JSON event is classified into exactly one lane:

| Lane | Routing Criteria | Channel | Capacity |
|------|-----------------|---------|----------|
| **LiveState** | `UpdateState`, `ClockUpdated` | `tokio::sync::watch` | Single value |
| **EventFeed** | `EventFeedMarker`, `MatchHistoryMarker` | `tokio::sync::mpsc` | 2,048 |
| **Historical** | `Goal`, `Save`, `Demolition` | `tokio::sync::mpsc` | 8,192 |

Unrecognised event types are assigned to `EventFeed` as a safety default with type `Unknown(String)`.
A monotonic sequence number is assigned to every event at classification time, forming an
`IngestEnvelope { seq, event_type, payload, class, active_match_id }`.

## Stage 3: Context

`SessionContext` extracts or generates session identifiers and injects them into the outbound payload:

1. Extract `match_id` from the event payload (attempts keys: `match_guid`, `match_id`, `matchGUID`).
2. Extract `session_id` from the event payload (attempts keys: `session_id`, `sessionId`).
3. If neither is present, fall back to deterministic timestamps:
   - `match_id = "match_{timestamp_ms}"`
   - `session_id = "session_{timestamp_ms}"`

Identifiers persist across events within a session. A session boundary is detected when a new
`match_guid` or `match_id` appears in the event payload without a matching `session_id`.

## Stage 4: Normalization

The normalizer handles two wire-format families:

- **camelCase:** `playerTelemetry`, `gameSecondsRemaining`, `attackerId`
- **snake_case:** `player_telemetry`, `game_seconds_remaining`, `attacker_id`

Normalization is lane-specific:

| Lane | Normalized Fields |
|------|-------------------|
| LiveState | `time`, `score` (home/away), player telemetry (recursive tree walk) |
| EventFeed | `timestamp`, `game_seconds_remaining`, `type`, `attacker_id`, `victim_id` |
| Historical | `timestamp`, `game_seconds`, `type`, `player_id`, `details` sub-object |

Player telemetry collection uses a recursive depth-first traversal over the entire JSON payload,
aggregating all key-value pairs under any object with a `player_name` or `playerName` key.

## Stage 5: Sink Actors

Three independent tokio tasks consume from their respective channels and push to Firebase.

### LiveState Actor

- **Channel:** `watch::Receiver<AppState>`
- **Deduplication:** Compares `seq` against the last-sent sequence; skips if no change.
- **Firebase route:** `PUT /live_state.json`
- **Delivery:** Best-effort. Network failures are logged and discarded.

### EventFeed Actor

- **Channel:** `mpsc::Receiver<IngestEnvelope>` (capacity 2,048)
- **Backpressure:** `try_send` — drops the oldest message when full.
- **Retry policy:** Maximum 3 attempts per event. After 3 failures, the event is discarded.
- **Firebase route:** `POST /live_events_feed.json`

### Historical Actor

- **Channel:** `mpsc::Receiver<IngestEnvelope>` (capacity 8,192)
- **Backpressure:** `send().await` — blocks the producer when full (lossless).
- **Retry policy:** Infinite retry with exponential backoff (full jitter).

#### Exponential Backoff with Full Jitter

```
base = 1s
max = 32s
for attempt in 0.. {
    cap = min(max, base * 2^attempt)
    sleep = random_uniform(0, cap)
    await sleep
}
```

The jitter uses nanosecond-precision randomness to avoid thundering-herd effects. A `Terminal` error
(schema mismatch, auth failure) causes immediate drop with no retry. A `RateLimited` error backs off
by an extra second. A `TransientNetwork` error follows the standard backoff schedule.

## Reliability Guarantees

```
LiveState:   at most once   │  Fire-and-forget, overwrite semantics
EventFeed:   at most once   │  Drop-when-full, 3-retry limit
Historical:  at least once  │  Infinite retry, lossless bounded queue

Critical invariant: backpressure on the Historical lane never stalls
                     the LiveState or EventFeed lanes.
```

The three-lane design ensures that a Firebase outage or rate-limit on historical data does not
block real-time state updates or feed markers from reaching the UI or cloud dashboards.
