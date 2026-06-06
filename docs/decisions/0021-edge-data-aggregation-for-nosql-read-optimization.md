# ADR 0021: Edge Data Aggregation for NoSQL Read Optimisation

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0021 |
| **Title** | Edge Data Aggregation for NoSQL Read Optimisation |
| **Status** | Accepted |
| **Date** | 2026-06-05 |

## Context
The companion mobile client (Kotlin) renders two views that require historical match data: a match-results list and per-player stat progression. Without preprocessing, the mobile app would have to query `matches_events_history/{match_id}` — the raw historical event node storing every `GoalScored`, `Save`, `StatfeedEvent`, and `Demolition` event for each match. A typical competitive match generates hundreds of such events, each a full JSON object with timestamps, player identities, and nested payloads.

Downloading this node for match-history display is a NoSQL anti-pattern colloquially known as a "Fat Read": the client retrieves kilobytes of data to extract three or four scalar values (final score, goals, saves). For a user with dozens of completed matches, the cumulative bandwidth cost becomes measurable on both the Firebase billing side and the mobile device's radio power budget.

ADR 0019 established lifecycle-triggered compaction to keep transient nodes clean, but compaction only purges data — it does not restructure it for read efficiency. A complementary write-time transformation was needed to precompute the lightweight summaries that the mobile client actually queries, so that reads become O(1) scalar lookups rather than O(n) document scans.

## Decision
We implement **Edge Data Aggregation** within the Rust daemon's ingestion router. At match transition, immediately after the flush barriers complete (ADR 0020) and before the compaction `DELETE` requests fire (ADR 0019), the router extracts a snapshot of the `live_state` actor's accumulated `master_state` and builds denormalised summary objects. These are uploaded to dedicated Firebase RTDB nodes via concurrent HTTP `PUT` requests.

### Snapshot Extraction
The `live_state` actor's `master_state` is local to the actor task and not accessible externally. To retrieve it without violating the actor ownership model, a new `Snapshot` variant is added to the `TransientLaneMessage` enum used by both transient lanes:

```rust
pub enum TransientLaneMessage {
    Event(IngestEnvelope),
    Flush { ack: oneshot::Sender<()> },
    Snapshot { result: oneshot::Sender<Value> },
}
```

The router calls `request_live_state_snapshot`, which sends a `Snapshot` message through the existing `mpsc` channel and awaits the response via `oneshot`. The ordering guarantee from ADR 0020 holds: the snapshot message is enqueued after the `Flush` message, ensuring the actor has drained all pre-flush `UpdateState` events and the returned `master_state` reflects the final accumulated state for the concluding match.

### Summary Object Schema
Two lightweight denormalised objects are built from the snapshot:

**`MatchIndexEntry`** — written to `matches_index/{match_id}`:
| Field | Type | Source |
|-------|------|--------|
| `timestamp` | `u64` | `SystemTime::now()` (Unix seconds) |
| `blue_score` | `u64` | `state.score.blue` |
| `orange_score` | `u64` | `state.score.orange` |
| `match_id` | `String` | Router's `previous_match_id` |

**`PlayerMatchLog`** — written to `player_match_logs/{sanitized_player}/{match_id}`:
| Field | Type | Source |
|-------|------|--------|
| `timestamp` | `u64` | `SystemTime::now()` (Unix seconds) |
| `goals` | `u64` | `state.player_telemetry.{player}.goals` |
| `shots` | `u64` | `state.player_telemetry.{player}.shots` |
| `saves` | `u64` | `state.player_telemetry.{player}.saves` |
| `assists` | `u64` | `state.player_telemetry.{player}.assists` |
| `score` | `i64` | `state.player_telemetry.{player}.score` |
| `touches` | `u64` | `state.player_telemetry.{player}.touches` |
| `demos` | `u64` | `state.player_telemetry.{player}.demos` |
| `match_id` | `String` | Router's `previous_match_id` |

Player IDs (e.g., Steam names) may contain characters illegal in Firebase RTDB path segments: `.`, `#`, `$`, `/`, `[`, `]`. The `sanitize_firebase_key` function replaces these with underscores (`_`) before constructing the upload path.

### Concurrent Upload via `join_all`
The `put_node` method on the `EventSink` trait (mirrored on `FirebaseConnector`) accepts a path and a `&Value`, performing an HTTP `PUT` with JSON body to the RTDB REST API. Because each summary object is independent — `matches_index/{match_id}` plus one `player_match_logs/{player}/{match_id}` per player — all uploads are issued concurrently using `futures_util::future::join_all`.

```
┌──────────────────────┐
│ flush_transient_lanes │  (ADR 0020)
└─────────┬────────────┘
          │
          ▼
┌──────────────────────┐
│ request_live_state    │
│ _snapshot             │
└─────────┬────────────┘
          │
          ▼
┌──────────────────────┐
│ build_match_index     │
│ build_player_logs     │
└─────────┬────────────┘
          │
          ▼
┌──────────────────────────────────────┐
│ join_all(                            │
│   put_node("matches_index/..."),     │
│   put_node("player_match_logs/..."), │
│   put_node("player_match_logs/..."), │
│   ...                                │
│ )                                    │
└─────────┬────────────────────────────┘
          │
          ▼
┌──────────────────────┐
│ compact_transient     │  (ADR 0019)
│ _nodes (DELETE)       │
└──────────────────────┘
```

### Ghost Match Filtering
Match transitions triggered by `IdTransition` (ADR 0019) can fire when BakkesMod transitions from a pre-match lobby to the actual match — the fallback session ID changes to the real GUID, but the `live_state` actor has never received player telemetry for the lobby. Before invoking the concurrent upload, the router checks whether the snapshot's `player_telemetry` object is non-empty. Ghost matches with zero players are silently skipped; no `matches_index` or `player_match_logs` entries are created.

### Retry Policy and Failure Mode
Each per-path upload carries an independent retry budget of 3 attempts (`AGGREGATION_MAX_FAILURES`), using the same full-jitter exponential backoff algorithm as the historical lane (ADR 0008). Terminal failures (`4xx` client errors) abort immediately. Rate-limited and transient failures retry up to the budget, then log and discard. Per ADR 0012, aggregation upload failures are fail-soft: a failed `put_node` for one player's log does not prevent other players' logs or the match index from being uploaded, nor does it block the downstream compaction `DELETE`.

### Lifecycle Event Routing Correction
Lifecycle events (`MatchEnded`, `MatchDestroyed`, `MatchInitialized`) no longer route to the `event_feed` lane. ADR 0019's compaction `DELETE` clears the `/live_events_feed` node at match transition; if the lifecycle event that triggered compaction were also routed to `event_feed`, it would arrive *after* the delete and leave a single orphaned event in the feed. These events now route exclusively to the historical lane, preventing post-compaction contamination. `MatchDestroyed` — previously not mirrored to historical — now routes there as well, ensuring it is preserved.

## Rejected Alternatives
- **Client-side aggregation on the mobile device:** Rejected because it requires the Fat Read this ADR exists to eliminate. The mobile client would still download every raw historical event to compute summaries locally, defeating the bandwidth and battery optimisation.
- **GCP Cloud Function triggered by RTDB writes:** Rejected because it introduces an external infrastructure dependency with cold-start latency and Firebase Admin SDK credential management. The Rust daemon already holds the authoritative in-memory state at match conclusion; offloading aggregation to a separate runtime adds cost without adding capability.
- **Aggregate on the historical lane actor rather than at compaction time:** Rejected because the historical actor processes individual events and has no visibility into the "match concluded" lifecycle signal. Only the ingestion router — which observes `MatchEnded`/`MatchDestroyed` events and triggers compaction — knows with certainty when a match is complete and its `master_state` is final.
- **Store aggregated objects via `send_event` on a new lane:** Rejected because it would require a new lane actor and channel, and the summary objects are not telemetry events — they are database write operations at the persistence layer. Using `put_node` directly keeps the `EventSink` trait as the single database I/O surface.

## Consequences

### Positive
- **O(1) mobile reads:** The mobile client queries `matches_index/{match_id}` (a few dozen bytes) instead of `matches_events_history/{match_id}` (potentially tens of kilobytes). Match-results list and stat-progression views become single-key scalar lookups.
- **Reduced Firebase billing:** Bandwidth costs scale with bytes downloaded. Eliminating the Fat Read for match-history queries directly reduces monthly RTDB egress proportionally to the number of matches viewed on mobile.
- **Lower mobile radio power consumption:** Fewer bytes transferred means shorter cellular-radio active time per query, reducing battery drain on metered connections.
- **No external infrastructure:** Aggregation runs entirely within the daemon process at the moment the data is already in memory. No additional services, credentials, or network round-trips to a processing layer.
- **Piggybacks on existing flush guarantees:** By inserting aggregation between the flush barrier (ADR 0020) and compaction (ADR 0019), the implementation inherits the mathematical ordering guarantee that `master_state` is final for the concluded match without introducing new synchronisation primitives.

### Negative / Limitations
- **Transient CPU and network burst at match transition:** The daemon serialises and uploads `1 + N` objects concurrently (one match index plus one log per player) at the exact moment a match ends. For a 6-player match, this is 7 concurrent HTTP `PUT` requests. The `join_all` pattern blocks the router until all uploads complete, adding latency to the compaction cycle.
- **RTDB key sanitisation surface:** Player IDs contain platform-specific prefixes (`Steam|`, `Epic|`, `PsyNet|`) and display names can include RTDB-illegal characters. The `sanitize_firebase_key` function in Rust must stay in sync with RTDB path rules. A newly introduced illegal character in a future game update could silently produce invalid paths and failed uploads.
- **No historical backfill:** Aggregation only fires for matches concluded while the daemon is running. Matches played before this ADR was implemented, or during daemon downtime (crashes, system sleep), have no summaries. The mobile client must fall back to the Fat Read for those matches or display a degraded view.
- **Stat granularity is coarse:** The `PlayerMatchLog` captures end-of-match totals. Per-minute stat progression or intra-match leaderboard snapshots are not available from the aggregation layer; those queries still require a full scan of `matches_events_history`.

### Mitigations
- The ghost-match filter prevents spurious uploads for lobby transitions, keeping the number of aggregation writes proportional to actual gameplay.
- Aggregation uploads use independent retry budgets and `join_all` concurrency — a transient failure on one path does not cascade to others.
- The `AGGREGATION_MAX_FAILURES` constant (3) is colocated with `COMPACTION_MAX_FAILURES` for operational visibility. Both can be tuned via recompilation.
- Lifecycle events are now excluded from the `event_feed` lane, preventing the orphaned-event contamination that would otherwise defeat the compaction guarantees of ADR 0019.
