# ADR 0013: Deterministic Fallback Identification for Missing Context

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0013 |
| **Title** | Deterministic Fallback Identification for Missing Context |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Certain telemetry frames may arrive without explicit match_id or session_id due to transient session context loss, startup race conditions, or upstream schema gaps. Without identity fields, downstream routing and storage cannot reliably associate events with analytical partitions. However, discarding structurally valid telemetry solely because context keys are absent reduces dataset completeness and weakens longitudinal statistics.

## Decision
When match_id or session_id is missing, the system deterministically generates fallback identifiers using local machine timestamp-derived values, preserving event admissibility and pipeline continuity.

## Rejected Alternatives
- Discarding contextless events: rejected because it unnecessarily sacrifices potentially valuable telemetry and biases aggregate statistics.
- Blocking ingestion until identity context appears: rejected because it introduces avoidable pipeline stalls and availability degradation.
- Generating non-deterministic random IDs without temporal structure: rejected because operational traceability and post-hoc correlation become harder.

## Consequences

### Positive
- Maintains ingestion continuity under temporary context loss.
- Preserves otherwise valid telemetry for downstream analysis and auditing.

### Negative / Limitations
- Generated identifiers may not naturally correlate across disconnected sessions.
- Fallback identity paths require explicit downstream awareness to avoid over-interpreting synthetic linkage.
