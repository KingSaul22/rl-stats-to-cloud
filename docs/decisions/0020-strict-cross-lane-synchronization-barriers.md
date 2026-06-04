# ADR 0020: Strict Cross-Lane Synchronization Barriers

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0020 |
| **Title** | Strict Cross-Lane Synchronization Barriers |
| **Status** | Accepted |
| **Date** | 2026-06-04 |

## Context
In ADR 0001 we established a three-lane pipeline: LiveState, EventFeed, and Historical, each backed by independent channel primitives. In ADR 0019 we introduced lifecycle-triggered database compaction: when the ingestion router observes a match transition, it fires HTTP `DELETE` requests to wipe the `/live_state` and `/live_events_feed` RTDB nodes.

These two designs collide on a race condition. When the router detects a match boundary and initiates compaction, the transient lane actors (LiveState and EventFeed) may still have buffered messages in their inbound `mpsc` channels — envelopes classified and enqueued *before* the match transition event, but not yet processed by their respective actor tasks. If the `DELETE` hits Firebase before these buffered envelopes are drained, the following sequence occurs:

1. Router enqueues `MatchEnded` to EventFeed lane.
2. Router classifies `MatchEnded` as a compaction trigger.
3. Router fires `DELETE /live_state`, `DELETE /live_events_feed`.
4. The LiveState actor drains its queue and `PUT`s live state to `/live_state`.
5. The EventFeed actor drains its queue and `PUT`s feed events to `/live_events_feed`.

Steps 4 and 5 write data to nodes that should be empty for the new match, creating **ghost data** — stale transient state from the previous match bleeding into the new match's clean database namespace. Mobile clients querying RTDB see a mix of old and new match data until the next compaction cycle.

Additionally, the original LiveState lane used a `tokio::sync::watch` channel (ADR 0001). A `watch` channel has no drain semantics — the receiver holds the last value and overwrites it on each send. There is no mechanism to ask the watch channel "are you empty?" because the concept of emptiness does not apply. This makes flush-barrier coordination impossible for the LiveState lane under the watch primitive.

## Decision
We implement **strict cross-lane synchronization barriers** with two coordinated changes:

### 1. Migrate LiveState from `watch` to `mpsc` Channel
The LiveState lane channel is migrated from `tokio::sync::watch` to `tokio::sync::mpsc(2048)` — the same bounded `mpsc` primitive used by EventFeed. The `TransientLaneMessage` enum (shared by both transient lanes) wraps either an event envelope or a control message:

```rust
pub enum TransientLaneMessage {
    Event(IngestEnvelope),
    Flush { ack: oneshot::Sender<()> },
}
```

The LiveState actor's semantics are preserved through its processing logic: it maintains an in-memory `master_state` cache and overwrites it on each received event, emitting only the latest merged state to Firebase. The `mpsc` channel provides transient buffering (for absorption of short bursts) while retaining the "send latest value" behaviour via the actor's internal overwrite logic.

### 2. Introduce `Flush` Control Message with `oneshot` Acknowledgement
Before the ingestion router executes compaction `DELETE` requests, it sends a `TransientLaneMessage::Flush { ack }` to both the LiveState and EventFeed lane senders. Each actor, on receiving the `Flush` message, immediately fires `ack.send(())` — signaling that all messages enqueued *before* the `Flush` have been drained from the channel and processed by the actor.

The router performs this in two phases:

**Phase 1 — Enqueue Flush:**
```
send_flush_request("live_state", sender, ack_tx)  // with timeout
send_flush_request("event_feed", sender, ack_tx)  // with timeout
```

The `send_flush_request` call uses `tokio::time::timeout` (2-second `COMPACTION_FLUSH_TIMEOUT`) to avoid deadlocking if the lane's `mpsc` channel is full or closed. If the send times out or the channel is closed, the failure is logged and compaction proceeds anyway — trading a rare missed flush for guaranteed progress.

**Phase 2 — Await Acknowledgements:**
```
wait_for_flush_ack("live_state", ack_rx)   // with timeout
wait_for_flush_ack("event_feed", ack_rx)   // with timeout
```

Each `wait_for_flush_ack` call `.await`s the `oneshot::Receiver<()>` with the same 2-second timeout. Successful receipt of the unit `()` value guarantees the actor's queue is empty of all pre-flush events.

Only after both ACKs are received (or timed out) does the router proceed to fire the `DELETE` requests against Firebase.

## Rejected Alternatives
- **Insert a `tokio::time::sleep` delay before compaction:** Rejected because it provides no mathematical guarantee of queue emptiness. The correct delay is unknowable — it depends on channel depth, network latency, and sink backpressure. Any fixed delay is either too short (fails) or too long (adds unnecessary latency).
- **Drop and recreate channels on match transition:** Rejected because dropping a channel closes it permanently. The actor tasks would need to be respawned, adding complexity and opening a window where events are lost during the actor restart.
- **Keep `watch` channel and add a separate flush coordination channel:** Rejected because it bifurcates the coordination surface. The `watch` channel has no ordering guarantees relative to a second control channel, so messages on the `watch` and the flush signal can be reordered arbitrarily.
- **Move compaction to the actor tasks themselves:** Rejected because the actors operate on single lanes and have no visibility into match lifecycle events (those are dispatched via the EventFeed lane). The router is the only component with cross-lane visibility and the authority to sequence flush-then-delete.

## Consequences

### Positive
- **Mathematical guarantee of queue emptiness:** The `Flush` / `oneshot` ACK pattern provides a happens-before relationship: all events enqueued before the flush message are processed before the ACK fires. The compaction `DELETE` is guaranteed to execute on an empty transient queue for both lanes.
- **No data bleed between matches:** Ghost data from a previous match cannot appear in the new match's transient nodes, eliminating cross-match contamination.
- **Bounded worst-case latency:** The 2-second `COMPACTION_FLUSH_TIMEOUT` bounds the worst-case wait for flush completion while preventing indefinite hangs from a stalled or dead actor.
- **Graceful degradation under failure:** Both `send_flush_request` and `wait_for_flush_ack` treat timeout and channel closure as non-fatal. The router always proceeds to compaction even if a lane fails to acknowledge, ensuring the pipeline never deadlocks.
- **Unified lane channel type:** Both transient lanes now use `mpsc<TransientLaneMessage>`, simplifying the routing infrastructure and enabling uniform flush logic across lanes.

### Negative / Limitations
- **Minor latency during match transitions:** The router must `.await` up to 2 seconds for the flush ACKs before executing the `DELETE`. This adds a small but measurable delay to the compaction cycle. During this window, events for the new match may already be arriving and buffering behind the flush in the same channel — the actor processes them immediately after the flush ACK, so they are not lost, merely deferred.
- **Increased LiveState memory footprint:** The `watch` channel stored exactly 1 value. The `mpsc(2048)` channel pre-allocates slot capacity for up to 2048 messages. This is a minor regression in memory efficiency, though bounded by the hard channel cap.
- **Imperfect under extreme backpressure:** If an actor's `mpsc` queue is full and the send times out, the `Flush` message is never enqueued and the compaction proceeds without the guarantee. This is a known trade-off: we accept the risk of rare ghost data over the risk of pipeline deadlock.

### Mitigations
- The `COMPACTION_FLUSH_TIMEOUT` (2 seconds) is tuned to be generous for normal operation while preventing pipeline stalls. It can be adjusted via recompilation if field data shows it is too aggressive or too conservative.
- Both actors handle `Flush` as their first `match` arm, ensuring it is processed with minimal overhead (a single `ack.send(())` call).
- The `last_compaction_seq` guard from ADR 0019 ensures the flush + compact sequence fires at most once per match boundary, preventing cascading flush storms.
