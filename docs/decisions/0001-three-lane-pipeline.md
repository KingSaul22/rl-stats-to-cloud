# ADR 0001: Three-Lane Telemetry Pipeline

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0001 |
| **Title** | Three-Lane Telemetry Pipeline |
| **Status** | Accepted |
| **Date** | 2025-05-01 |
| **Author** | Saul |
| **Supersedes** | — |

## Context

The Core Service ingests Rocket League telemetry from a local game source at high frequency
(events per second can spike during match phases). Each event falls into one of three semantic
categories:

1. **Live State** — the current scoreboard and player telemetry snapshot (overwrites, not
   accumulates). Low individual value per event; only the latest matters.
2. **Event Feed** — match-level markers such as period starts, match start/end, clock updates.
   Moderate value; occasional loss is tolerable.
3. **Historical Events** — goals, saves, demolitions. High value; every event must reach Firebase
   to maintain accurate match statistics.

A naive single-queue architecture would couple the latency and reliability of all three categories.
A transient Firebase outage or rate-limit on historical writes would stall the entire pipeline,
delaying live state updates to the dashboard and preventing feed markers from reaching cloud
monitoring. The user-facing UI would freeze during what should be a localized backpressure event.

Additionally, the three categories have fundamentally different delivery semantics:

- LiveState is a "latest value wins" problem — a `watch` channel is the natural fit.
- EventFeed is a "best effort" stream — a bounded `mpsc` with drop-when-full semantics is
  appropriate.
- Historical is a "must not lose" stream — an `mpsc` with blocking `.await` send is required.

## Decision

We will split the ingestion pipeline into three independent lanes, each backed by a different
channel primitive appropriate to its delivery semantics.

### Lane Specification

```
┌─────────────┐     ┌─────────────────┐     ┌──────────────┐
│ Game Source │────►│ Classification  │────►│ LiveState    │ watch, lossy
│             │     │                 │────►│ EventFeed    │ mpsc(2048), lossy
│             │     │                 │────►│ Historical   │ mpsc(8192), lossless
└─────────────┘     └─────────────────┘     └──────────────┘
```

| Lane | Channel | Capacity | Send Behaviour | Retry Policy |
|------|---------|----------|----------------|--------------|
| LiveState | `tokio::sync::watch` | 1 | Overwrite | Best-effort (no retry) |
| EventFeed | `tokio::sync::mpsc` | 2,048 | `try_send` (drop) | Max 3 retries |
| Historical | `tokio::sync::mpsc` | 8,192 | `send().await` (block) | Infinite, full-jitter backoff |

### Rationale for Channel Selection

- **LiveState → `watch`:** Only the most recent value matters. A `watch` channel naturally
  overwrites the old value with the new. No queueing, no memory growth, no backpressure.
- **EventFeed → `mpsc(2048), try_send`:** A bounded buffer absorbs short bursts. `try_send`
  drops when full, preventing a slow Firebase from blocking the Ingestion Engine. Capacity 2,048
  provides reasonable burst absorption without excessive memory use.
- **Historical → `mpsc(8192), send().await`:** Async send applies backpressure to the engine
  when the queue fills, but does not drop data. Capacity 8,192 is sized to absorb transient
  network interruptions before backpressure engages.

### Retry Policy: Full Jitter Exponential Backoff

All sink actors use the same backoff algorithm, but with different retry ceilings:

```
base = 1s
max = 32s
cap = min(max, base * 2^attempt)
sleep = random_uniform(0, cap)
```

Full jitter (uniform random in `[0, cap]`) is chosen over decorrelated jitter or equal jitter
because it provides the lowest probability of continuous contention across multiple sink actors
retrying simultaneously, per the AWS Architecture Blog's analysis of backoff strategies.

## Consequences

### Positive

1. **Lane independence:** A Firebase outage affecting historical writes never blocks the live
   state dashboard. Users always see current connection status and last event.
2. **Appropriate semantics per lane:** Each lane uses the primitive that matches its data's
   importance and access pattern. No "one size fits all" queue compromise.
3. **Bounded memory:** The LiveState watch channel never grows. EventFeed and Historical have
   hard caps that prevent unbounded memory growth during extended outages.
4. **Predictable latency:** LiveState has zero queuing latency. EventFeed has bounded queuing
   delay. Historical backpressure is isolated and does not propagate upstream to the other lanes.
5. **Testable in isolation:** Each sink actor can be tested independently with mock sinks,
   enabling unit tests for retry logic, deduplication, and backpressure behaviour.

### Negative

1. **Code complexity:** Three actor tasks, three channel types, and lane-specific normalization
   paths increase the total code surface. The classification stage must route correctly or events
   land in the wrong lane.
2. **Ordering divergence:** Events across lanes may arrive at Firebase out of chronological order.
   A goal (Historical) sent 5 seconds ago may arrive after a live state update (LiveState) sent
   just now, because Historical is retrying and LiveState is not. Consumers must tolerate
   eventual consistency.
3. **Configuration burden:** Two mpsc capacities (2,048 and 8,192) are hardcoded. Tuning these
   for different deployment environments requires recompilation.
4. **Duplicate risk:** The Historical lane's at-least-once semantics mean that a successful
   Firebase write followed by a network partition during the HTTP response could result in a
   retry and duplicate event. Firebase Realtime DB's `POST` with server-generated keys mitigates
   but does not eliminate this risk. Application-level idempotency keys are a future enhancement.

### Mitigations

- Classification routing is tested with exhaustive match arms on `RocketLeagueEvent`.
- Unknown event types fall back to `EventFeed` (the safest, lowest-stakes lane).
- Historical duplication risk is acknowledged and deferred to a future ADR on idempotency.
