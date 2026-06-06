# Operations

## Starting the Core Service

The Core Service runs as a foreground process and must be started before the Tauri desktop client.

```bash
cargo run -p rl_stats_core
```

The process binds:

| Endpoint | Address | Protocol |
|----------|---------|----------|
| IPC Control Server | `127.0.0.1:43210` | TCP, JSON |
| UI Sync Server | `127.0.0.1:{ui_sync_port}` | WebSocket (lazy, on demand) |

The Core Service will remain active until it receives a graceful shutdown signal
(`Ctrl+C` or `--poweroff`).

## CLI Control Commands

All control commands are sent to the running Core Service's IPC Control Server (`127.0.0.1:43210`).
Run them from the workspace root.

```bash
# Request the Core Service to start the UI Sync Server
cargo run -p rl_stats_core -- --allow-ui

# Request the Core Service to stop the UI Sync Server (ingestion continues)
cargo run -p rl_stats_core -- --disallow-ui

# Request a graceful shutdown of the entire Core Service
cargo run -p rl_stats_core -- --poweroff
```

Each command spawns a short-lived process that connects to the IPC Control Server, transmits a
one-line JSON command, reads the JSON reply, and exits. A 3-second timeout governs the exchange.

### Expected Replies

| Command | Reply | Meaning |
|---------|-------|---------|
| `AllowUi` | `{ "status": "Ok" }` | UI Sync Server started |
| `AllowUi` | `{ "status": "Error" }` | Server could not start (port bound?) |
| `DisallowUi` | `{ "status": "Ok" }` | Server stopped |
| `DisallowUi` | `{ "status": "NotRunning" }` | Server was not active |
| `Poweroff` | (connection closed) | Graceful shutdown acknowledged |

## Launching the UI

```bash
bun run tauri dev
```

On startup, the Tauri thin client:

1. Loads `config.json` from the platform config directory.
2. Sends `AllowUi` to the IPC Control Server.
3. Connects to the UI Sync Server at `ws://127.0.0.1:{ui_sync_port}`.
4. Subscribes to state updates, rendering `AppState` (connection status, last event,
   lane routing statistics) in the dashboard.

## UI Sync Server Auto-Timeout

The UI Sync Server has an idle timeout of **30 seconds** (`UI_IDLE_AUTO_DISALLOW_SECONDS`).
If no WebSocket client is connected for that duration, the server shuts down to conserve resources.
The Tauri client re-requests `AllowUi` on reconnect.

This means the dashboard may briefly show a "reconnecting" state when idle — this is normal.

## Graceful Shutdown

1. **Tauri client closes:** sends `DisallowUi` to the IPC Control Server.
2. **Core Service receives SIGINT:** cancels all tokio tasks, drains in-flight sink operations,
   unbinds the control and UI sync listeners, then exits.

## Running Headless

Set `"isHeadless": true` in `config.json`. The Core Service will not start the UI Sync Server
in response to `AllowUi` commands, and the Tauri app will display an offline diagnostics panel.

## Configuration File

Location: platform-specific config directory (see [Configuration](architecture.md#configuration)).
Created automatically on first launch with sensible defaults. Edit via the Tauri app's configuration
form or by hand.

## Observability

### Logging Infrastructure

The daemon uses the **`tracing`** crate backed by **`tracing-subscriber`**
(`core/src/main.rs:11` — `tracing_subscriber::fmt::init()`). This captures both explicit
tracing macros and standard stdout/stderr output.

**Structured tracing** (`warn!`, `error!`) is used for:

- Lane routing diagnostics (backpressure drops, channel-closed errors)
- Aggregation errors (serialization failures, cumulative stats read/write failures)
- Compaction warnings (snapshot timeouts, flush barrier failures)

**Operational `println!` / `eprintln!`** remain for:

- Startup and shutdown lifecycle messages
- Connection state transitions (connected, disconnected, retrying)
- CLI command acknowledgments
- Sink warnings (Firebase HTTP errors, rate limits)
- IPC and UI server status

All output (tracing macros and standard print) is captured by the `tracing-subscriber` `fmt`
layer and rendered to stderr.

### Runtime Statistics

The UI Sync Server broadcasts an `AppState` snapshot that includes per-session lane routing
counters exposed by `RoutingStats`:

| Counter | Description |
|---------|-------------|
| `event_feed_losses` | Events dropped from the EventFeed channel (backpressure) |
| `historical_overflows` | Events dropped from the Historical channel (backpressure) |
| `live_state_drops` | LiveState events dropped (backpressure) |

These counters are visible in the Tauri dashboard and are reset on each reconnection.

## CI/CD Guardrail: Blocking Socket Detection

The `scripts/check-no-sync-socket-async.sh` script enforces a strict invariant: **no blocking
`std::net` or `std::io` socket APIs in async runtime paths.** It uses `ripgrep` to scan
`core/src/daemon/`, `core/src/worker/`, and `src-tauri/src/bridge/` (excluding the synchronous
IPC client at `core/src/daemon/client.rs`, which is intentionally blocking).

Failing this check (exit code 1) blocks the CI pipeline. See
[Validation Commands](development.md#validation-commands) for integration.
