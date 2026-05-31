# ADR 0008: Full Jitter Exponential Backoff for Network Retries

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0008 |
| **Title** | Full Jitter Exponential Backoff for Network Retries |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Telemetry sinks operate over unreliable networks where transient outages and remote throttling are expected operational states rather than exceptional edge cases. During recovery windows, synchronized retries from many queued events can produce burst amplification, creating self-inflicted congestion and prolonged instability. Retry policy must therefore optimize not only eventual success probability but also aggregate system behavior under correlated failure.  
A decision was needed for a retry algorithm that minimizes thundering-herd effects while preserving bounded retry cadence.

## Decision
Transient network retries use full jitter exponential backoff, parameterized as random(0, min(max, base * 2^attempt)).

## Rejected Alternatives
- Fixed-interval retries (for example, every 5 seconds): rejected because synchronized retry waves repeatedly hammer upstream services and extend outage recovery time.
- Deterministic exponential backoff without jitter: rejected because clients remain phase-aligned, preserving herd behavior at larger intervals.
- Immediate aggressive retries: rejected because they consume local resources and worsen downstream saturation during incident periods.

## Consequences

### Positive
- De-correlates retry timing across queued events, reducing contention and recovery shock.
- Improves systemic stability during partial outages and rate-limit episodes.

### Negative / Limitations
- Some individual events may observe longer stochastic wait times than fixed-interval policies.
- Retry observability and debugging require probabilistic interpretation rather than deterministic schedules.
