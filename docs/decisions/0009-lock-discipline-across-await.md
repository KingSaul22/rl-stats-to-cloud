# ADR 0009: Lock Discipline Across Await Boundaries

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0009 |
| **Title** | Lock Discipline Across Await Boundaries |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Asynchronous Rust runtimes rely on cooperative scheduling, where tasks yield at await points. Holding a Mutex or RwLock guard across await can indefinitely delay competing tasks, creating hidden priority inversions and deadlock patterns under load. In a long-running telemetry daemon with concurrent ingestion, routing, and sink tasks, lock misuse can convert localized latency into global throughput collapse.  
A strict concurrency policy was required to preserve progress guarantees and maintain deterministic behavior under contention.

## Decision
No Mutex or RwLock guard is held across await points. Critical sections remain short-lived, and data required for asynchronous operations is copied or cloned out of the lock before awaiting.

## Rejected Alternatives
- Guarding async network I/O with shared locks: rejected because lock hold-time becomes network-dependent, enabling deadlock and starvation scenarios.
- Coarse-grained global locking for simplicity: rejected because it serializes unrelated work and destroys pipeline parallelism.
- Relaxed lock discipline with code-review-only enforcement: rejected because policy ambiguity increases regression risk in evolving async code.

## Consequences

### Positive
- Strong reduction in deadlock risk and lock-contention amplification.
- Preserves concurrent throughput by keeping lock scope tightly bounded in time.

### Negative / Limitations
- Requires disciplined engineering practices and occasional data cloning overhead.
- May increase local code verbosity where lock extraction and state handoff are explicit.
