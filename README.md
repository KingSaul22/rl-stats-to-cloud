# RL Stats to Cloud Workspace

This repository is a Cargo workspace with a daemon + client architecture.

## Workspace Layout

- core (`rl_stats_core`): background service daemon and control plane.
- src-tauri (`rl-stats-to-cloud`): Tauri desktop UI acting as a thin client.

## Binaries

### rl_stats_core (Daemon)

The `rl_stats_core` binary runs as a long-lived foreground daemon that:

- Owns the Rocket League worker lifecycle.
- Exposes a local IPC control endpoint.
- Can start/stop a local WebSocket UI sync server (`ws://127.0.0.1:{ui_sync_port}`).

### rl-stats-to-cloud (Tauri UI)

The Tauri app does not spawn the worker anymore.
It sends `AllowUi` to the daemon and subscribes to daemon state updates over WebSocket, then emits `status-update` to the frontend.

## Daemon Control Commands

Run from the workspace root:

```bash
cargo run -p rl_stats_core
```
Starts the daemon.

```bash
cargo run -p rl_stats_core -- --allow-ui
```
Requests the daemon to start the local UI WebSocket server.

```bash
cargo run -p rl_stats_core -- --disallow-ui
```
Requests the daemon to stop the local UI WebSocket server without stopping the worker.

```bash
cargo run -p rl_stats_core -- --poweroff
```
Requests graceful daemon shutdown.

## Configuration

Core configuration is stored in `config.json` and includes:

- `websocket_url`
- `connector`
- `reconnect_delay_seconds`
- `ui_sync_port`

`ui_sync_port` is backward compatible with older config files via serde defaults.

## Running the UI

```bash
bun run tauri dev
```

The UI reads configuration, asks daemon to allow UI streaming, and reconnects to `ws://127.0.0.1:{ui_sync_port}` if disconnected.

## Validation Commands

```bash
cargo check --workspace
```

```bash
cd core
cargo deny check advisories
```

## Optional Quality Checks

```bash
cargo clippy --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
```

## Update cargo dependencies
```bash
cargo outdated && cargo upgrade && cargo update && cargo check
```