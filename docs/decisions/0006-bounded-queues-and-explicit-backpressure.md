# ADR 0006: Bounded Queues and Explicit Backpressure Management

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0006 |
| **Title** | Bounded Queues and Explicit Backpressure Management |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Telemetry ingestion operates continuously under variable network and cloud conditions. During sink degradation, buffering behavior determines whether the system remains stable or accumulates unbounded in-memory state. In long-lived daemons, unbounded buffering transforms transient outages into deterministic memory exhaustion risk.  
A queueing policy decision was required to preserve memory safety and operational predictability under adverse transport conditions.

## Decision
All internal buffering structures are bounded with explicit capacity limits, and each lane implements deliberate backpressure behavior (drop or block) rather than unbounded accumulation.

## Rejected Alternatives
- Unbounded channels (for example, unbounded mpsc): rejected because they permit uncontrolled memory growth under sink slowdown and can lead to OOM termination.
- Implicit buffering without explicit lane-level policy: rejected because it obscures overload behavior and prevents deterministic reliability analysis.

## Consequences

### Positive
- Memory usage remains bounded and analytically predictable under sustained stress.
- Overload behavior becomes explicit, testable, and aligned with lane-specific reliability objectives.

### Negative / Limitations
- Requires careful capacity sizing and continuous tuning based on workload characteristics.
- Necessitates explicit policy handling for pressure scenarios (drops in lossy lanes, blocking in durable lanes).
