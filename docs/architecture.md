# Architecture

## Service Boundaries

RL Stats to Cloud is split into two decoupled processes sharing no memory, communicating exclusively
over local TCP and WebSocket.

### Core Service (`rl_stats_core`)

A long-lived foreground process responsible for all data-plane work. It owns:

- **Ingestion Engine:** Opens and maintains a connection to the game telemetry source
  (`ws://127.0.0.1:49123` by default). Handles reconnection with configurable delay.
- **Sink Actors:** Three independent async tasks that push classified, normalized events to Firebase.
- **Session Context:** Extracts or generates `match_id` and `session_id` per game session and
  injects them into outbound payloads.
- **Payload Normalizer:** Recursively collects player telemetry, handles both `camelCase` and
  `snake_case` wire formats.
- **IPC Control Server:** Listens on `127.0.0.1:43210` (TCP). Accepts line-delimited JSON commands
  and replies with JSON responses. Used by the Tauri thin client and CLI tooling.
- **UI Sync Server:** A WebSocket server on `127.0.0.1:{ui_sync_port}` that streams `AppState`
  snapshots (connection status, last event type) to connected clients. Auto-disallows after 30
  seconds of zero clients.

The Core Service is compilable as both a binary (`cargo run -p rl_stats_core`) and a library
(consumed by the Tauri client for shared types).

### Thin Client (`rl-stats-to-cloud`)

A Tauri v2 desktop application. It performs no data-plane work. Responsibilities:

- Loads and persists `config.json` from the platform config directory.
- Sends `AllowUi` / `DisallowUi` commands to the Core Service via IPC.
- Subscribes to the UI Sync Server WebSocket for live state updates.
- Emits `status-update` Tauri events to the WebView frontend.
- Exposes three Tauri commands to the TypeScript frontend: `get_config`, `save_config`, `get_status`.

### Frontend (WebView SPA)

A TypeScript single-page application built with Vite 6 and Zod 4 for runtime schema validation.
Renders connection status, last event, offline diagnostics, and a configuration form.

## Inter-Process Communication

```
┌──────────────┐                        ┌──────────────────┐
│  Thin Client │  ── TCP (43210) ──►    │  Core Service    │
│  (Tauri)     │  AllowUi/DisallowUi    │                  │
│              │                        │                  │
│              │  ◄── WebSocket ──      │  UI Sync Server  │
│              │      (54321)           │  (state push)    │
│              │      AppState JSON     │                  │
└──────────────┘                        └──────────────────┘
```

### IPC Control Server (TCP, port 43210)

The control protocol uses newline-delimited JSON over raw TCP:

**Command (request):**
```json
{ "AllowUi": null }
{ "DisallowUi": null }
{ "Poweroff": null }
```

**Reply:**
```json
{ "status": "Ok" }
{ "status": "NotRunning" }
{ "status": "Error" }
```

The Thin Client sends `AllowUi` on startup and `DisallowUi` on graceful shutdown. CLI tools
(`--allow-ui`, `--disallow-ui`, `--poweroff`) issue the same commands via a synchronous TCP client
over the same endpoint. A 3-second timeout applies to all responses.

### UI Sync Server (WebSocket, dynamic port)

Once the IPC Control Server receives `AllowUi`, the Core Service binds a WebSocket server on
`127.0.0.1:{ui_sync_port}` (default 54321). Each connected client receives:

1. An immediate snapshot of the current `AppState`.
2. Subsequent pushes whenever `AppState` changes (connection status, last event).

The server auto-terminates if no client connects for 30 seconds (`UI_IDLE_AUTO_DISALLOW_SECONDS`).

## Port Allocation

| Port | Protocol | Owner | Purpose |
|------|----------|-------|---------|
| 49123 | WebSocket or TCP | External (game) | Telemetry ingestion source |
| 43210 | TCP | Core Service | IPC Control Server |
| 54321 | WebSocket | Core Service | UI Sync Server (configurable) |
| 1420 | HTTP | Vite dev server | Frontend HMR (dev only, strict) |

Port 49123 supports both WebSocket and raw TCP connections. The Ingestion Engine auto-detects
the protocol and falls back from WebSocket to TCP if the initial HTTP handshake fails.

## Configuration

`config.json` is created on first run at the OS-specific config path:

| Platform | Path |
|----------|------|
| Windows  | `%APPDATA%\rl-stats-to-cloud\config.json` |
| Linux    | `$XDG_CONFIG_HOME/rl-stats-to-cloud/config.json` |
| macOS    | `$HOME/Library/Application Support/rl-stats-to-cloud/config.json` |

### `AppConfig` Schema

```json
{
  "isHeadless": false,
  "websocketUrl": "ws://127.0.0.1:49123",
  "connector": {
    "Firebase": {
      "url": "https://<project>.firebaseio.com",
      "authToken": "<secret>"
    }
  },
  "reconnectDelaySeconds": 5,
  "uiSyncPort": 54321
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `isHeadless` | `bool` | `false` | Suppress UI Sync Server entirely |
| `websocketUrl` | `string` | `"ws://127.0.0.1:49123"` | Telemetry source endpoint |
| `connector.Firebase.url` | `string` | — | Firebase Realtime Database URL |
| `connector.Firebase.authToken` | `string` | — | Firebase auth secret |
| `reconnectDelaySeconds` | `u64` | `5` | Seconds before reconnection attempt |
| `uiSyncPort` | `u16` | `54321` | Port for the UI Sync Server |
