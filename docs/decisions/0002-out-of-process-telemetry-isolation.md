# ADR 0002: Out-of-Process Telemetry and Process Boundary Isolation

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0002 |
| **Title** | Out-of-Process Telemetry and Process Boundary Isolation |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The telemetry source is the Rocket League Native Stats API, which broadcasts JSON events over a local WebSocket/TCP connection directly from the game engine. External cloud communication introduces non-deterministic delays due to network jitter, transient packet loss, DNS resolution, TLS handshakes, and remote service throttling. Performing those operations directly from the game process would risk coupling cloud responsiveness to gameplay-critical paths, potentially impacting frame pacing and render-thread stability.  
A decision was therefore required to enforce fault containment between gameplay-critical telemetry extraction and best-effort cloud transport.

## Decision
The game engine emits telemetry natively over a local WebSocket/TCP endpoint. All external network I/O is delegated to a separate Rust daemon process. The game process performs no direct cloud writes.

## Rejected Alternatives
- In-process HTTP communication from the game client to Firebase: rejected because it couples gameplay-critical timing to unpredictable network latency and failure modes.
- Direct cloud transport from the game client with retry logic: rejected because retries amplify blocking windows and increase the probability of frame-time interference under outage conditions.

## Consequences

### Positive
- Strong failure isolation between gameplay runtime and cloud transport subsystem.
- Stable in-game frame pacing independent of cloud responsiveness.

### Negative / Limitations
- Requires an additional daemon lifecycle on the user host (startup, supervision, and shutdown).
- Introduces inter-process transport complexity and operational dependency on local IPC availability.
