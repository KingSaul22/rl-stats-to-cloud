# Data Pipeline

The ingestion pipeline is a seven-stage, multi-lane architecture designed to prevent slow or failing
sinks from blocking time-sensitive telemetry. Events are classified, normalized, and routed across
three independent lanes with cross-lane mirroring for critical lifecycle events.

## Data Flow

```
                         ┌──────────────────────┐
                         │     Worker           │
                         │  (session loop,      │
                         │   auto-reconnect)    │
                         └──────────┬───────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
             ┌──────────┐  ┌──────────────┐  ┌──────────┐
             │LiveState │  │  EventFeed   │  │Historical│
             │mpsc(2048)│  │ mpsc(2048)   │  │mpsc(8192)│
             │coalesced │  │  try_send    │  │try_send  │
             └────┬─────┘  └──────┬───────┘  └────┬─────┘
                  │               │               │
                  ▼               ▼               ▼
             ┌──────────┐  ┌──────────────┐  ┌──────────┐
             │  Sink    │  │    Sink      │  │  Sink    │
             │ Best-eff.│  │ max 3 retry  │  │∞ retry   │
             └────┬─────┘  └──────┬───────┘  └────┬─────┘
                  │               │               │
                  ▼               ▼               ▼
             ┌───────────────────────────────────────────┐
             │              Firebase Realtime DB          │
             └───────────────────────────────────────────┘
```

During match transitions, the pipeline enforces **flush barriers** on the LiveState and EventFeed
lanes. The Worker records a snapshot of the accumulated `master_state`, runs post-match aggregation
(match index, player logs, cumulative stats), cleans transient Firebase nodes, and resets the
state cache before the next match begins.

## Stage 1: Ingestion

The Worker opens a persistent connection to `websocketUrl` (default `ws://127.0.0.1:49123`).
Two transport modes are supported:

| Mode | Detection | Parser |
|------|-----------|--------|
| WebSocket | Default | `tokio-tungstenite` |
| TCP (raw) | Fallback on HTTP parse error | Line-delimited JSON stream |

On connection loss, the Worker sleeps for `reconnectDelaySeconds` (default 5) and retries
indefinitely. Connection state changes are broadcast via `set_connected(bool)`, which feeds both
the UI Sync Server and the `live_state` lane.

If a sink actor channel is detected as closed (`TrySendError::Closed`), the error propagates as
a fatal session error, triggering immediate reconnection with full lane rebuild. See
[Fail-Fast Error Propagation](#fail-fast-error-propagation).

## Stage 2: Event Classification

Each parsed JSON payload carries an `Event` field whose value is matched against a known enum
of Rocket League telemetry types. Classification is a deterministic `match` that assigns every
event to exactly one primary lane:

| Lane | Routing Criteria | Channel | Capacity |
|------|-----------------|---------|----------|
| **LiveState** | `UpdateState`, `ClockUpdatedSeconds` | `tokio::sync::mpsc` | 2,048 |
| **EventFeed** | `StatfeedEvent`, `GoalReplay*`, `MatchCreated`, `MatchInitialized`, `MatchEnded`, `MatchDestroyed`, `MatchPaused`, `MatchUnpaused`, `CountdownBegin`, `RoundStarted`, `PodiumStart`, `ReplayCreated`, `BallHit`, `CrossbarHit` | `tokio::sync::mpsc` | 2,048 |
| **Historical** | `GoalScored`, unknown/unrecognised event types | `tokio::sync::mpsc` | 8,192 |

Unrecognised event types are assigned to `Historical` as a safety default with type `Unknown(String)`.
A monotonic sequence number is assigned to every event at classification time, forming an
`IngestEnvelope { seq, event_type, payload, class, active_match_id }`.

## Stage 3: Context Resolution

`SessionContext` extracts or generates session identifiers and injects them into the outbound payload:

1. Extract `match_id` from the event payload (attempts keys: `match_guid`, `match_id`, `matchGUID`).
2. Extract `session_id` from the event payload (attempts keys: `session_id`, `sessionId`).
3. If neither is present, fall back to deterministic timestamps:
   - `match_id = "match_{timestamp_ms}"`
   - `session_id = "session_{timestamp_ms}"`

Identifiers persist across events within a session. A session boundary is detected when a new
`match_guid` or `match_id` appears in the event payload without a matching `session_id`.

Additionally, `game_seconds_remaining` is extracted from the raw payload and cached in the router
for enrichment of lifecycle events that lack timing data (see [Mirroring Rules](#mirroring-rules)).

## Stage 4: Payload Normalization

The normalizer handles two wire-format families:

- **camelCase:** `playerTelemetry`, `gameSecondsRemaining`, `attackerId`
- **snake_case:** `player_telemetry`, `game_seconds_remaining`, `attacker_id`

Normalization is lane-specific:

| Lane | Normalized Fields |
|------|-------------------|
| LiveState | `time`, `score` (blue/orange), player telemetry (recursive tree walk) |
| EventFeed | `timestamp`, `game_seconds_remaining`, `type`, `attacker_id`, `victim_id` |
| Historical | `timestamp`, `game_seconds`, `type`, `player_id`, `details` sub-object |

Player telemetry collection uses a recursive depth-first traversal over the entire JSON payload,
aggregating all key-value pairs under any object with a `player_name` or `playerName` key.

## Stage 5: Compaction & Match Finalization

When the router detects a match boundary — `MatchDestroyed`, `MatchEnded`, or an active
match ID transition — it triggers a compaction sequence that finalizes the previous match
and prepares for the next one.

### Trigger Detection

```
MatchDestroyed    → CompactionReason::Destroyed
MatchEnded        → CompactionReason::Ended
match_id changes  → CompactionReason::IdTransition
```

The trigger compares the *current* `active_match_id` against the *previous* `active_match_id`
(captured before `SessionContext::update_from_payload` mutates it). If a compaction was already
performed for the current sequence number, it is skipped (idempotency guard).

### Flush Barriers

Before aggregation or cleanup can proceed, all in-flight events on the LiveState and EventFeed
lanes must be drained. A `Flush` message carrying a oneshot acknowledgment is enqueued behind
all pending events on each lane. The router awaits both acknowledgments with a 2-second timeout
(`COMPACTION_FLUSH_TIMEOUT`). If a channel is closed, the session aborts.

### Post-Match Aggregation

After the flush, the router requests a snapshot of the accumulated `master_state` from the
LiveState actor. If the snapshot contains player telemetry and the match has not already been
aggregated (guarded by `last_aggregated_match_id`), the aggregation engine runs:

1. **Build `MatchIndexEntry`:** Extracts scores, timestamps, and denormalized team stat totals
   (shots, saves, assists, demos per side) computed from player telemetry.
2. **Resolve team identities:** The majority-rule roster resolution queries the `/players`
   registry in Firebase for each player on each side. See [Roster Resolution](#majority-rule-roster-resolution).
3. **Build `PlayerMatchLog`:** Per-player match logs recording goals, shots, saves, assists,
   score, touches, and demos.
4. **Update cumulative stats:** Read-modify-write of `stats_cumulative/{player_id}` and
   `stats_cumulative_teams/{team_id}` with full jitter exponential backoff.
5. **Upload `MatchIndexEntry`:** The enriched match index (containing resolved team IDs and
   denormalized stat totals) is written to `matches_index/{match_id}`.

### Transient Node Cleanup

After aggregation, the compaction routine clears the two transient Firebase nodes
(`live_state` and `live_events_feed`) by overwriting them with empty objects. This prevents
cross-match data bleed. The cleanup is retried up to `COMPACTION_MAX_FAILURES` (3) times
with exponential backoff.

## Stage 6: Cross-Lane Routing & Mirroring

After compaction (if any) and normalization, the envelope is routed to one or more lanes
based on its classification and the current router state.

### Routing Lanes

| Primary Lane | Routing Behaviour |
|-------------|-------------------|
| **LiveState** | Always forwarded to the LiveState channel. Drops silently if the channel is full (backpressure). |
| **EventFeed** | Forwarded to EventFeed channel unless the event is a lifecycle event (`MatchEnded`, `MatchDestroyed`, `MatchInitialized`) AND `podium_active` is true. |
| **Historical** | Always forwarded to the Historical channel. Drops silently on backpressure (channel full). |

### Synchronization Latches

Two in-memory boolean latches gate cross-lane routing to prevent engine memory bleed and ghost
lobbies during match transitions:

| Latch | Set True On | Set False On |
|-------|-------------|--------------|
| `cached_podium_active` | `MatchEnded` event | Compaction after `IdTransition` or `Destroyed` |
| `cached_historical_active` | `MatchInitialized` event | Compaction after `IdTransition` or `Destroyed` |

- **Podium gate:** During podium (`podium_active == true`), lifecycle events are suppressed from
  the EventFeed lane to prevent ghost lobby events from appearing in the live feed.
- **Historical gate:** Aggregation only runs if `historical_active` is true, preventing partial
  data from matches that never properly initialized.
- **Replay gate:** Events received while `in_replay` is true are skipped entirely for Historical
  classification to avoid double-counting stats during goal replays.

### Mirroring Rules

Certain events are *mirrored* — routed to an additional lane beyond their primary classification:

**High-value mirroring (bidirectional):**

Events classified as high-value (`GoalScored`, `Goal`, `StatfeedEvent`, `MatchInitialized`,
`MatchEnded`, `PodiumStart`) are mirrored in both directions:
- EventFeed-classified events → also enqueued to Historical
- Historical-classified events → also enqueued to EventFeed (unless lifecycle)

**Lifecycle mirroring (EventFeed → Historical):**

Match lifecycle events (`MatchEnded`, `MatchDestroyed`, `MatchInitialized`) are mirrored to
Historical while `historical_active` is true, ensuring a complete match chronology in the
historical record.

**Timing enrichment:** Mirrored historical copies are enriched with the cached
`game_seconds_remaining` value if their payload lacks it, ensuring accurate timeline
placement even for events that do not carry timing data.

### Fail-Fast Error Propagation

When any channel is detected as **closed** (the actor task has terminated, returning
`TrySendError::Closed`), the routing function returns an `Err`. This error propagates through
the call chain:

```
try_send_*  →  route_envelope  →  handle_value  →  handle_payload
    →  run_websocket_session / run_raw_tcp_session  →  run_session
        →  main Worker loop (drop lanes, reconnect)
```

The main Worker loop catches the error, logs it, marks the connection as disconnected,
drops all lanes (which terminates the actor tasks), sleeps for `reconnectDelaySeconds`,
and establishes a fresh session with new channels and actors. This pattern ensures that
a deadlocked or panicked actor never leaves the pipeline in a zombie state.

### Deduplication Gate

Deduplication of match transitions relies on the router's internal `previous_match_id` field
(extracted from `session_context.active_match_id` *before* it is updated by the incoming
payload), not on untrusted JSON payload IDs. This prevents spoofed or replayed events from
triggering false compaction boundaries.

## Stage 7: Sink Actors

Three independent tokio tasks consume from their respective channels and push to Firebase.

### LiveState Actor

- **Channel:** `mpsc::Receiver<TransientLaneMessage>` (capacity 2,048)
- **Coalescing:** On each new `Event` message, the actor drains the channel with `try_recv`
  in a tight loop, accumulating state merges from the drained events and discarding older
  duplicates. Only the final merged `master_state` is sent to Firebase.
- **Match transition:** When the `active_match_id` changes, the master state is reset to an
  empty object, preventing cross-match data bleed.
- **Firebase route:** `PUT /live_state.json`
- **Delivery:** Best-effort. Network failures are logged and discarded.

### EventFeed Actor

- **Channel:** `mpsc::Receiver<TransientLaneMessage>` (capacity 2,048)
- **Backpressure:** `try_send` — drops the oldest message when full.
- **Retry policy:** Maximum 3 attempts per event. After 3 failures, the event is discarded.
- **Firebase route:** `POST /live_events_feed.json`

### Historical Actor

- **Channel:** `mpsc::Receiver<IngestEnvelope>` (capacity 8,192)
- **Backpressure:** `try_send` — drops the event when full. The producer logs a warning
  with the accumulated drop counter and continues. This prevents ingestion stalls when
  Firebase is slow or rate-limited.
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

A `Terminal` error (schema mismatch, auth failure) causes immediate drop with no retry.
A `RateLimited` error backs off by an extra second. A `TransientNetwork` error follows the
standard backoff schedule.

## Majority-Rule Roster Resolution

During post-match aggregation, the system must determine which registered eSports team
occupied each side (blue/orange). The algorithm works as follows:

1. For each player on a side, query `/players/{player_id}/team_id` from Firebase.
2. Count votes: each successful lookup contributes one vote for its `team_id`.
3. Apply strict majority rule: a `team_id` is selected if and only if its count is
   strictly greater than every other `team_id`'s count and greater than zero.
4. If no majority exists, a deterministic fallback ID is generated:
   `temp_{match_id}_blue` or `temp_{match_id}_orange`.

This approach naturally accommodates substitute players — as long as a majority of
players on a side share a registered `team_id`, the resolution is correct regardless
of who the remaining players are registered to. Unregistered players (no `team_id`
in the registry) cast no vote.

## NoSQL Read Optimization

The `MatchIndexEntry` struct (stored at `matches_index/{match_id}`) serves as a
dual-purpose index node. In addition to scores and timestamps, it carries denormalized,
pre-computed team stat totals:

| Field | Description |
|-------|-------------|
| `blue_team_id` / `orange_team_id` | Resolved team identities (majority-rule, or `null`) |
| `blue_shots` / `orange_shots` | Sum of all per-side player shot counts |
| `blue_saves` / `orange_saves` | Sum of all per-side player save counts |
| `blue_assists` / `orange_assists` | Sum of all per-side player assist counts |
| `blue_demos` / `orange_demos` | Sum of all per-side player demolition counts |

These fields are computed once during aggregation via `sum_team_stats` and consumed by
both the match index upload and the cumulative team stats update. The Kotlin mobile
client can query a single RTDB path (`matches_index?orderBy="blue_team_id"&equalTo="eclipse_total"`)
to render per-match team progression charts without additional reads.

## Reliability Guarantees

```
LiveState:   at most once   │  Coalesced mpsc, overwrite semantics
EventFeed:   at most once   │  Drop-when-full, 3-retry limit
Historical:  at least once  │  Infinite retry, lossless bounded queue

Critical invariant: backpressure on the Historical lane never stalls
                     the LiveState or EventFeed lanes.
```

The three-lane design ensures that a Firebase outage or rate-limit on historical data does not
block real-time state updates or feed markers from reaching the UI or cloud dashboards.
