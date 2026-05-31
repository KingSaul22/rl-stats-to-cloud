# ADR 0004: Tauri over Electron for the Desktop Operational Client

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0004 |
| **Title** | Tauri over Electron for the Desktop Operational Client |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The desktop application is an auxiliary operational surface (monitoring, configuration, diagnostics), not the primary data-plane engine. During gameplay, the client may remain idle for extended periods and must minimize CPU residency, memory pressure, and background contention with game processes.  
A framework decision was required to ensure the UI layer does not compromise host resource availability in performance-sensitive gaming scenarios.

## Decision
The desktop client is implemented with Tauri (Rust backend plus OS-native WebView) rather than Electron.

## Rejected Alternatives
- Electron: rejected because shipping a bundled Chromium + Node.js runtime imposes avoidable baseline RAM/CPU overhead for a mostly idle control interface.
- Browser-only external dashboard: rejected because the project requires local operational integration and tight IPC interaction with the daemon.

## Consequences

### Positive
- Significantly lower idle resource footprint and smaller distribution artifacts.
- Better alignment with a utility-class background telemetry architecture.

### Negative / Limitations
- Increased sensitivity to platform-specific WebView behavior and rendering differences.
- Requires careful cross-platform UI validation due to host engine heterogeneity.
