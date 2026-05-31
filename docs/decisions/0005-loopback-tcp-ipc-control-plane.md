# ADR 0005: Loopback TCP IPC Control Plane

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0005 |
| **Title** | Loopback TCP IPC Control Plane |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The architecture requires a control channel between the daemon and desktop UI for lifecycle commands and operational state coordination. The IPC mechanism must remain cross-platform, implementation-simple, and compatible with asynchronous Rust networking primitives.  
A decision was needed between portable loopback networking and platform-dependent local IPC mechanisms that introduce conditional code paths and tooling complexity.

## Decision
The control plane is implemented as loopback TCP bound to 127.0.0.1:43210, using async tokio networking primitives.

## Rejected Alternatives
- Named Pipes (Windows) and Unix Domain Sockets (Linux/macOS): rejected due to OS-specific divergence, increased build/test matrix complexity, and maintenance overhead.
- Hybrid IPC abstraction via additional interprocess layers: rejected because it adds dependency and portability complexity disproportionate to control-plane requirements.

## Consequences

### Positive
- Uniform IPC behavior and implementation model across operating systems.
- Straightforward observability and troubleshooting using standard TCP tooling.

### Negative / Limitations
- Possibility of local port collisions with other applications.
- TCP framing and timeout handling must be explicitly managed at the application protocol layer.
