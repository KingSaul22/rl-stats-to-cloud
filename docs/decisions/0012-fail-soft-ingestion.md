# ADR 0012: Fail-Soft Ingestion Under Malformed Telemetry

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0012 |
| **Title** | Fail-Soft Ingestion Under Malformed Telemetry |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
Telemetry ingress is exposed to upstream instability, including malformed JSON frames, partial payloads, and unknown event names introduced by plugin or game-version drift. In a continuous ingestion system, strict fail-stop behavior transforms isolated data-quality defects into full service outages, violating availability requirements and interrupting all lanes regardless of data criticality. A robust ingestion architecture must therefore contain bad inputs while preserving forward progress for valid telemetry.

## Decision
The ingestion engine is fail-soft: malformed frames are rejected without terminating the daemon, and unrecognized events are quarantined to low-risk handling paths or safely dropped according to lane policy.

## Rejected Alternatives
- Fail-stop panic on malformed telemetry: rejected because single-frame corruption would crash the daemon and cause disproportionate availability loss.
- Global session termination on parse errors: rejected because localized input faults should not reset healthy ingestion state.
- Blind acceptance of malformed payloads: rejected because downstream persistence would accumulate inconsistent and analytically harmful data.

## Consequences

### Positive
- High operational uptime despite noisy or partially invalid upstream telemetry.
- Fault containment at ingress prevents localized schema defects from propagating into system-wide failure.

### Negative / Limitations
- Some invalid or unknown events are intentionally discarded, requiring strong observability to quantify data loss.
- Error-handling paths and quarantine logic increase implementation and testing complexity.
