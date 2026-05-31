# ADR 0007: Semantic Channel Primitive Selection by Data Value

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0007 |
| **Title** | Semantic Channel Primitive Selection by Data Value |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The telemetry pipeline transports data classes with distinct reliability and freshness semantics. LiveState updates are overwrite-dominant and lose utility rapidly, EventFeed markers are best-effort observational signals, and Historical events contribute to long-term statistical integrity. A uniform buffering primitive would force one compromise across incompatible quality-of-service requirements, either wasting memory on stale state snapshots or under-protecting durable statistics.  
A concurrency design decision was therefore required to align channel behavior with domain semantics, bounded memory policy, and predictable backpressure outcomes.

## Decision
Channel primitives are selected per lane semantics: watch for LiveState (latest-value overwrite), bounded mpsc with try_send for EventFeed (lossy under pressure), and bounded mpsc with send().await for Historical (blocking admission when full).

## Rejected Alternatives
- Using bounded mpsc uniformly for all lanes: rejected because queueing stale LiveState entries inflates memory pressure without informational benefit.
- Using watch uniformly for all lanes: rejected because overwrite semantics would silently discard non-reconstructable historical events.
- Using unbounded channels to avoid lane-specific policy: rejected because it violates bounded-memory guarantees and hides overload behavior.

## Consequences

### Positive
- Channel mechanics match information value, yielding efficient memory usage and explicit reliability behavior per lane.
- Prevents cross-lane interference by encoding backpressure policy at the transport primitive level.

### Negative / Limitations
- Increases implementation complexity through lane-specific actor logic and test matrices.
- Requires careful documentation to avoid semantic drift between routing policy and sink behavior.
