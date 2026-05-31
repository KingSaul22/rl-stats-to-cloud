# ADR 0014: Headless-First Runtime Design

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0014 |
| **Title** | Headless-First Runtime Design |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The primary product responsibility is continuous telemetry ingestion, not interactive visualization. User-facing windows are operationally optional and may be closed for long durations during gameplay. If ingestion liveness depends on UI process state, telemetry reliability becomes coupled to unrelated presentation-layer failures, including renderer crashes, window closure, and frontend reload events. A production telemetry architecture therefore requires independent daemon autonomy.

## Decision
The Rust daemon is designed to operate fully independently of the desktop UI, sustaining ingestion and cloud synchronization in headless mode.

## Rejected Alternatives
- Binding daemon lifecycle to UI window lifecycle: rejected because closing or crashing the UI would terminate telemetry collection and degrade user experience.
- Running ingestion loops inside UI process context: rejected because presentation-layer instability would directly threaten data-plane availability.
- Requiring a permanently open dashboard for operation: rejected because it imposes unnecessary resource and interaction burden on end users.

## Consequences

### Positive
- True background-service behavior with ingestion continuity independent of UI state.
- Strong separation between mission-critical data plane and optional control surface.

### Negative / Limitations
- Requires explicit control and observability channels for users operating without an always-open UI.
- Introduces additional process orchestration responsibilities for startup and shutdown coordination.
