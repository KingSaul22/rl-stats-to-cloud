# ADR 0016: Frameworkless Vanilla TypeScript Frontend

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0016 |
| **Title** | Frameworkless Vanilla TypeScript Frontend |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The UI receives frequent operational updates and must remain lightweight while users run a resource-intensive game. In this context, runtime abstraction layers that perform repeated virtual tree reconciliation can add avoidable allocation churn and garbage-collection pressure, especially under near-real-time refresh cycles. The frontend scope is constrained and operational, favoring deterministic update paths over generalized component frameworks.

## Decision
The frontend is implemented with vanilla TypeScript and direct DOM manipulation, without React, Vue, or other virtual-DOM frameworks.

## Rejected Alternatives
- React/Vue virtual-DOM frameworks: rejected because high-frequency diffing and framework runtime overhead can increase GC activity and CPU consumption for a simple telemetry dashboard.
- Heavy SPA meta-frameworks with additional abstraction layers: rejected because they enlarge binary/runtime footprint without proportional functional benefit.
- Mixed framework and manual DOM model: rejected because dual paradigms increase maintenance complexity and risk inconsistent update semantics.

## Consequences

### Positive
- Lower runtime overhead and reduced memory pressure during continuous status updates.
- Direct, predictable rendering behavior suitable for operational dashboards.

### Negative / Limitations
- Manual UI synchronization logic requires stricter engineering discipline.
- Reduced framework ergonomics may increase implementation effort for future complex UI features.
