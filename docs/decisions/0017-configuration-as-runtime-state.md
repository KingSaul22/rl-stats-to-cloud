# ADR 0017: Configuration as Runtime State with External Persistence

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0017 |
| **Title** | Configuration as Runtime State with External Persistence |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
A telemetry platform deployed on heterogeneous user machines requires operationally adjustable parameters (for example, endpoints, ports, and connector options). Rebuilding binaries or forcing full process restarts for each parameter change increases operational friction, extends downtime windows, and degrades user trust in manageability. Configuration must therefore be externally persisted and synchronized through controlled runtime interfaces.

## Decision
Configuration is persisted in config.json and treated as mutable runtime state synchronized through IPC-aware control paths, enabling live operational adjustment without hardcoded constants.

## Rejected Alternatives
- Hardcoded configuration in application binaries: rejected because environment-specific tuning would require code changes and redeployment.
- Restart-required reconfiguration for routine parameter edits: rejected because it interrupts ingestion continuity and degrades operational UX.
- UI-only ephemeral configuration state: rejected because settings would be non-durable and diverge from daemon runtime truth across sessions.

## Consequences

### Positive
- Improves operational agility through persistent, externally managed configuration.
- Reduces service disruption by enabling runtime-aligned configuration updates.

### Negative / Limitations
- Requires robust synchronization semantics to prevent stale or conflicting in-memory state.
- Increases responsibility for configuration validation, compatibility checks, and safe rollout behavior.
