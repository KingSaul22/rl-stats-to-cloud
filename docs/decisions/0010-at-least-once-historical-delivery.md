# ADR 0010: At-Least-Once Delivery Semantics for Historical Events

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0010 |
| **Title** | At-Least-Once Delivery Semantics for Historical Events |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Historical telemetry (for example, goals, saves, and demolitions) constitutes the authoritative basis for downstream statistics and longitudinal analysis. Loss of such events irreversibly corrupts aggregate metrics and undermines analytical trustworthiness. Given expected transient network failures, the system must prefer durability over immediate completion for this lane.  
A reliability decision was therefore required to determine whether historical writes should prioritize throughput simplicity or data integrity.

## Decision
The Historical lane uses at-least-once semantics with infinite retry on transient failures.

## Rejected Alternatives
- At-most-once fire-and-forget delivery: rejected because temporary transport failures would permanently discard high-value statistical events.
- Bounded retry with eventual drop: rejected because retry exhaustion can still yield irreversible data loss during prolonged outages.
- Downgrading Historical to best-effort parity with EventFeed: rejected because it violates the semantic priority of durable match records.

## Consequences

### Positive
- Maximizes durability of critical historical telemetry under unstable network conditions.
- Preserves long-term statistical correctness by preventing silent loss of high-value events.

### Negative / Limitations
- Requires downstream tolerance for duplicates and idempotent interpretation strategies.
- Extended outage periods can increase queue residency time and delay eventual cloud visibility.
