# RL Stats to Cloud

A Cargo workspace with a daemon + Tauri desktop client for ingesting Rocket League telemetry data and pushing it to cloud storage (Firebase).

## Workspace Layout

```
Cargo workspace
├── core/   (rl_stats_core) — background service daemon and control plane
└── src-tauri/ (rl-stats-to-cloud) — Tauri v2 desktop UI (thin client)
```

The frontend lives at the workspace root: Bun + Vite + TypeScript + Zod SPA, served by Tauri's WebView.

## Architecture

```
Game Source (ws://127.0.0.1:49123)
        │
        ▼
┌───────────────────┐
│  rl_stats_core    │  ◄── TCP control ──  rl-stats-to-cloud (Tauri)
│  Daemon           │       43210            │
│                   │                        │
│  ┌─────────────┐  │  WebSocket (54321)     │
│  │ UI Sync WS  │──┼──────────────────────► │
│  │ Server      │  │   state updates        │
│  └─────────────┘  │                        │
│                   │                        ▼
│  ┌─────────────┐  │               ┌──────────────┐
│  │ Worker      │──┼──────────────►│  Firebase    │
│  │ (ingestion) │  │   HTTP/PUT    │  Realtime DB │
│  └─────────────┘  │               └──────────────┘
└───────────────────┘
```

### Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 49123 | WebSocket/TCP | Game telemetry source (default `websocket_url`) |
| 43210 | TCP | Daemon control plane (IPC) |
| 54321 | WebSocket | UI sync server (`ui_sync_port`) |
| 1420 | HTTP | Vite dev server (strict port) |

## Binaries

### `rl_stats_core` (Daemon)

Long-lived foreground daemon that:

- Owns the Rocket League worker lifecycle (reconnection, backpressure, graceful shutdown)
- Ingests game telemetry via WebSocket or raw TCP
- Classifies events into three lanes:
  - **LiveState** (watch channel, lossy) — latest state snapshot
  - **EventFeed** (mpsc, lossy when full) — feed markers, clock updates
  - **Historical** (mpsc, lossless) — goals, saves, demolitions
- Normalizes payloads (handles both camelCase and snake_case keys)
- Pushes events to Firebase via `FirebaseConnector` (PUT for live state, POST for event feed and historical)
- Exposes a TCP control endpoint at `127.0.0.1:43210` for IPC
- Can start/stop a WebSocket UI sync server at `127.0.0.1:{ui_sync_port}`
- Auto-disallows UI server after 30s with no connected clients

### `rl-stats-to-cloud` (Tauri UI)

Desktop application that:

- Loads and manages configuration (`config.json`)
- Sends `AllowUi` to the daemon via TCP IPC
- Subscribes to daemon state updates over WebSocket (`ws://127.0.0.1:{ui_sync_port}`)
- Emits `status-update` Tauri events to the frontend
- Exposes Tauri commands: `get_config`, `save_config`, `get_status`
- Gracefully sends `DisallowUi` on shutdown

### Frontend (TypeScript SPA)

- **Entry:** `index.html` → `src/main.ts`
- **Stack:** TypeScript 6, Vite 6, Zod 4, `@tauri-apps/api` 2
- **Modules:** `api.ts` (Tauri invoke wrapper), `ui.ts` (DOM rendering), `schemas.ts` (Zod runtime validation), `constants.ts`
- **Features:** Dashboard with connection status, live event display, offline diagnostics panel, configuration editor (connector type, Firebase credentials, WebSocket URL, UI sync port, reconnect delay, headless mode)

## Configuration

Stored in `config.json` at the platform-specific config directory (%APPDATA% on Windows, XDG on Linux/macOS). Schema via `AppConfig`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `isHeadless` | bool | `false` | Run without UI sync |
| `websocketUrl` | string | `"ws://127.0.0.1:49123"` | Game telemetry source |
| `connector` | object | — | `{ "Firebase": { "url": "...", "authToken": "..." } }` |
| `reconnectDelaySeconds` | u64 | `5` | Worker reconnect delay |
| `uiSyncPort` | u16 | `54321` | UI WebSocket sync port |

## Daemon Control Commands

Run from the workspace root:

```bash
# Start the daemon
cargo run -p rl_stats_core

# Start UI WebSocket server
cargo run -p rl_stats_core -- --allow-ui

# Stop UI WebSocket server (worker keeps running)
cargo run -p rl_stats_core -- --disallow-ui

# Graceful daemon shutdown
cargo run -p rl_stats_core -- --poweroff
```

The control commands connect to the daemon's TCP endpoint at `127.0.0.1:43210` and send a JSON command.

## Running the UI

```bash
bun run tauri dev
```

On startup, the Tauri app reads the config, sends `AllowUi` to the daemon, and subscribes to `ws://127.0.0.1:{ui_sync_port}` for state updates.

## Data Pipeline

1. **Ingestion:** `RocketLeagueWorker` connects to the game telemetry source, parses JSON stream
2. **Classification:** Each event is classified as `LiveState`, `EventFeed`, or `Historical`
3. **Context:** `SessionContext` extracts/injects `match_id` and `session_id`
4. **Normalization:** Payload keys are normalized (camelCase/snake_case agnostic), player telemetry collected
5. **Sink Actors:** Three independent async tasks with retry/backoff push to Firebase:
   - LiveState: deduplicated by sequence number, best-effort
   - EventFeed: max 3 failures then dropped
   - Historical: infinite retry with exponential backoff (full jitter, 1s–32s)

## Validation

```bash
# Type-check all crates
cargo check --workspace

# Security advisories (run from core/)
cargo deny check advisories

# Strict clippy (all + pedantic = deny, nursery = warn)
cargo clippy --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery

# Frontend type-check
bun run tsc
```

## Linting Configuration

- `core/clippy.toml` & `src-tauri/clippy.toml`: cognitive-complexity-threshold=10, too-many-arguments-threshold=4, type-complexity-threshold=50
- `src-tauri/rustfmt.toml`: max_width=100, fn_call_width=60, format_strings=true, reorder_imports=true
- Both crates: `unsafe_code = "forbid"`, `unwrap_used`/`expect_used`/`todo`/`panic` = deny

## Update Dependencies

```bash
cargo outdated && cargo upgrade && cargo update && cargo check
```

## Firebase Schemas

See `docs/firebase-base-schema.json` (v1) and `docs/firebase-base-schema_v2.json` (v2).
