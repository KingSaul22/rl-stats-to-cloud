# ADR 0015: UI as Observer, Not Authority

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0015 |
| **Title** | UI as Observer, Not Authority |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
In systems with strict process-boundary isolation, authoritative ingestion ownership must remain within the daemon to guarantee stability, consistent retry behavior, and controlled backpressure policies. Allowing the UI to own game WebSocket sessions or ingestion loops would blur responsibility boundaries, increasing failure coupling between transient frontend behavior and core telemetry durability. A clear control-observe separation is required to preserve fault isolation.

## Decision
The desktop UI acts as an observer and operator via IPC, consuming daemon state updates without owning the game connection, routing decisions, or ingestion execution loops.

## Rejected Alternatives
- Frontend directly managing game WebSocket ingestion: rejected because UI restarts, refreshes, or renderer failures would disrupt core telemetry flow.
- Shared ingestion ownership between UI and daemon: rejected because split authority complicates concurrency control and incident diagnosis.
- UI-side retry and buffering logic: rejected because reliability policies belong in the daemon where lane semantics and backpressure are centrally enforced.

## Consequences

### Positive
- Preserves process-boundary isolation and minimizes cross-layer failure propagation.
- Simplifies operational reasoning by keeping authoritative state transitions inside one runtime.

### Negative / Limitations
- Introduces IPC latency and serialization overhead between UI and daemon state views.
- Requires disciplined interface contracts to keep observer state consistent with daemon truth.
