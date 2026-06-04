# ADR 0019: Lifecycle-Triggered Database Compaction

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0019 |
| **Title** | Lifecycle-Triggered Database Compaction |
| **Status** | Accepted |
| **Date** | 2026-06-04 |

## Context
The Firebase Realtime Database (RTDB) is the authoritative sink for all three telemetry lanes. Two of the database nodes — `live_state` and `live_events_feed` — are semantically transient: they represent the current-match snapshot and recent event markers, carrying no value once a match concludes. Under continuous ingestion, these nodes grow unboundedly with stale data from every historical match. This has three negative effects:

1. **Bandwidth waste for mobile clients:** The companion mobile app queries the full RTDB tree. Stale transient nodes inflate payload sizes, increasing load times and data costs on metered connections.
2. **Billing impact:** Firebase RTDB charges proportionally to bytes downloaded. Stale data accumulates silently across sessions, driving up costs with no corresponding user value.
3. **Query noise:** Consumers of the `live_state` node must filter out entries from completed matches, complicating client logic.

Conversely, high-value analytics (goal events, save events, match metadata) must be preserved indefinitely. These are stored in the `matches_events_history` node, which is append-only and never subject to compaction.

A decision was required to automatically purge transient nodes at match boundaries, ensuring the database remains clean, small, and fast without manual intervention or external Cloud Functions.

## Decision
We implement **Lifecycle-Triggered Compaction** entirely within the Rust daemon's ingestion router. When the router observes a match lifecycle transition, it explicitly fires HTTP `DELETE` requests to wipe the transient Firebase nodes before the next match begins.

### Compaction Triggers
The router derives a `CompactionReason` from three conditions detected during event classification:

| Reason | Trigger |
|--------|---------|
| `Destroyed` | `RocketLeagueEvent::MatchDestroyed` event arrives |
| `Ended` | `RocketLeagueEvent::MatchEnded` event arrives |
| `IdTransition` | The `match_id` field in the current payload differs from the previous payload's `match_id` (both non-empty) — catches edge cases where `MatchEnded` or `MatchDestroyed` are missed due to packet loss |

### Compaction Targets
The daemon sends `DELETE` requests to exactly two Firebase RTDB paths:

- `/live_state`
- `/live_events_feed`

The `matches_events_history` node is never targeted for deletion. Historical analytics are preserved across match boundaries.

### Execution Flow

```
┌──────────────┐     CompactionReason detected?  ──► no ──► continue routing
│ Event Router │
└──────┬───────┘     yes
       │
       ▼
┌──────────────────┐
│ Flush transient   │  (see ADR 0020)
│ lane queues       │
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│ HTTP DELETE       │  → /live_state
│ per target        │  → /live_events_feed
│ (max 3 retries)   │
└──────────────────┘
```

Each `DELETE` is issued with the same retry policy used by the historical lane (full-jitter exponential backoff, capped at 3 attempts per target). A failed compaction is logged but does not block the ingestion pipeline — the daemon continues routing events for the next match.

### Sequence Deduplication
A `last_compaction_seq` counter prevents duplicate compaction attempts within the same event sequence. The compaction guard checks `last_compaction_seq < sequence` before executing, ensuring each match transition triggers compaction exactly once even when the router processes multiple classification events at the same boundary.

## Rejected Alternatives
- **External Cloud Function for scheduled cleanup:** Rejected because it adds operational dependency on a separate runtime (GCP Cloud Functions), requires Firebase Admin SDK credentials in cloud infrastructure, and introduces timing uncertainty — the function may run before or after the daemon has finished writing transient state for the current match.
- **Client-side (mobile app) filtering of stale data:** Rejected because it does not solve the bandwidth or billing problems; stale data is still downloaded, only to be discarded client-side.
- **Time-to-Live (TTL) via Firebase rules:** Rejected because Firebase RTDB does not natively support TTL-based expiration. The workaround (timestamp field + server-side script) is fragile and adds latency.

## Consequences

### Positive
- **Self-cleaning database:** Transient nodes are automatically purged at every match boundary, requiring zero manual maintenance or external infrastructure.
- **Reduced mobile payload sizes:** The `live_state` and `live_events_feed` nodes contain only the current match at any time, minimizing download size for companion apps.
- **Lower Firebase billing:** Fewer bytes served from RTDB directly reduces monthly costs proportional to the number of matches played.
- **Centralized lifecycle ownership:** The daemon is the single authority on match state, eliminating distributed coordination between compaction logic and ingestion logic.
- **Resilient to missing events:** The `IdTransition` fallback catch ensures compaction fires even when explicit `MatchEnded`/`MatchDestroyed` packets are lost, providing defense-in-depth against unreliable game WebSocket delivery.

### Negative / Limitations
- **Couples database hygiene to daemon uptime:** If the daemon is not running during a match transition (e.g., crash, system sleep), transient nodes are never cleaned. No background reconciliation path exists to clean up after missed transitions. Stale data accumulates until the daemon is restarted and observes the next match boundary.
- **Best-effort reliability:** Compaction `DELETE` requests use a finite retry budget (3 attempts). A prolonged Firebase outage or network partition during a match boundary may result in a failed compaction that is never retried. The transient nodes persist until the next match transition.
- **Destructive by design:** Once deleted, transient match state cannot be recovered. If a compaction fires spuriously (e.g., a false-positive `IdTransition` from a corrupted payload), the current match's live state is lost irreversibly.

### Mitigations
- The `IdTransition` guard requires both `previous_match_id` and `current_match_id` to be non-empty and different, preventing false positives from empty-string or uninitialized IDs.
- Compaction failures are logged with full context (sequence number, reason, match IDs) for post-hoc diagnosis.
- Future enhancement: a startup reconciliation pass that inspects the RTDB for orphaned transient data and cleans it on daemon boot (deferred to a separate ADR).
