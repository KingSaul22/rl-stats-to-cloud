# ADR 0002: Out-of-Process Telemetry and Process Boundary Isolation

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0002 |
| **Title** | Out-of-Process Telemetry and Process Boundary Isolation |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The telemetry source executes inside Rocket League through a BakkesMod C++ plugin, which operates in a latency-sensitive runtime where frame pacing and render-thread stability are strict non-functional requirements. External cloud communication introduces non-deterministic delays due to network jitter, transient packet loss, DNS resolution, TLS handshakes, and remote service throttling. If those operations occur in-process with the game plugin, network tail latency can propagate into gameplay performance degradation (e.g., frame-time spikes and FPS instability).  
A decision was therefore required to enforce fault containment between gameplay-critical telemetry extraction and best-effort cloud transport.

## Decision
Telemetry extraction remains inside the BakkesMod plugin, but all external network I/O is delegated to a separate Rust daemon process. The plugin emits telemetry over a local socket only; it performs no direct cloud writes.

## Rejected Alternatives
- In-process HTTP communication from the C++ plugin to Firebase: rejected because it couples render-time behavior to unpredictable network latency and failure modes.
- Direct cloud transport in the plugin with retry logic: rejected because retries amplify blocking windows and increase the probability of frame-time interference under outage conditions.

## Consequences

### Positive
- Strong failure isolation between gameplay runtime and cloud transport subsystem.
- Stable in-game frame pacing independent of cloud responsiveness.

### Negative / Limitations
- Requires an additional daemon lifecycle on the user host (startup, supervision, and shutdown).
- Introduces inter-process transport complexity and operational dependency on local IPC availability.
