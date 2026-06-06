# Architecture

## Service Boundaries

RL Stats to Cloud is split into two decoupled processes sharing no memory, communicating exclusively
over local TCP and WebSocket.

### Core Service (`rl_stats_core`)

A long-lived foreground process responsible for all data-plane work. It owns:

- **Ingestion Engine:** Opens and maintains a connection to the game telemetry source
  (`ws://127.0.0.1:49123` by default). Handles reconnection with configurable delay.
  Auto-detects WebSocket vs. raw TCP transport and falls back on handshake failure.
- **Classifer & Router:** Matches each parsed event against a known Rocket League event enum,
  assigns a primary lane (`LiveState`, `EventFeed`, or `Historical`), and applies cross-lane
  mirroring rules for lifecycle and high-value events.
- **Session Context:** Extracts or generates `match_id` and `session_id` per game session and
  injects them into outbound payloads. Guards against replay frames and match-ID spoofing.
- **Payload Normalizer:** Recursively collects player telemetry, handles both `camelCase` and
  `snake_case` wire formats, and enriches lifecycle events with cached game-clock timing.
- **Compaction Engine:** On match boundaries (`MatchEnded`, `MatchDestroyed`, match-ID
  transition), enforces flush barriers on the transient lanes, triggers post-match aggregation,
  clears transient Firebase nodes, and resets state caches.
- **Aggregation Engine:** Computes and uploads denormalized `MatchIndexEntry` (scores, resolved
  team IDs, per-side shot/save/assist/demo totals), per-player match logs, and cumulative
  player/team statistics. Uses majority-rule roster resolution against the `/players` registry.
- **Sink Actors:** Three independent async tasks that push classified, normalized events to Firebase
  with lane-specific retry policies (best-effort, 3-retry limit, or infinite retry with full
  jitter exponential backoff).
- **Fail-Fast Recovery:** If any actor's channel is detected as closed (`TrySendError::Closed`),
  the error propagates as a fatal session error, tearing down all lanes and triggering a clean
  reconnection with fresh channels and actors.
- **IPC Control Server:** Listens on `127.0.0.1:43210` (TCP). Accepts line-delimited JSON commands
  and replies with JSON responses. Used by the Tauri thin client and CLI tooling.
- **UI Sync Server:** A WebSocket server on `127.0.0.1:{ui_sync_port}` that streams `AppState`
  snapshots (connection status, last event type, lane routing statistics) to connected clients.
  Auto-disallows after 30 seconds of zero clients.

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
Renders connection status, last event, lane routing statistics, offline diagnostics, and a
configuration form.

## Data Plane

The three-lane pipeline is described in full in [`docs/pipeline.md`](pipeline.md). Key
architectural highlights:

| Lane | Channel | Delivery | Backpressure |
|------|---------|----------|-------------|
| **LiveState** | `mpsc` (2048), coalesced via `try_recv` drain | At-most-once, best-effort | Drops silently when full |
| **EventFeed** | `mpsc` (2048) | At-most-once, best-effort | Drops oldest when full |
| **Historical** | `mpsc` (8192) | At-least-once, lossless | Drops with warning when full |

### Match Finalization Pipeline

On every match boundary, the Worker executes a compaction sequence:

1. **Flush barriers** — drain in-flight events on LiveState and EventFeed lanes.
2. **State snapshot** — capture the accumulated `master_state` (scores, player telemetry).
3. **Aggregation** — build `MatchIndexEntry`, `PlayerMatchLog`s, resolve team identities
   via majority-rule roster lookup, update cumulative player/team stats.
4. **Transient cleanup** — overwrite `live_state` and `live_events_feed` Firebase nodes
   with empty objects.
5. **State reset** — clear the LiveState actor's cached master state for the next match.

### Denormalized Match Index

The `MatchIndexEntry` schema (stored at `matches_index/{match_id}`) is enriched with
edge-computed, denormalized fields to minimize downstream read costs:

```rust
struct MatchIndexEntry {
    timestamp: u64,
    blue_score: u64, orange_score: u64,
    match_id: String,
    blue_team_id: Option<String>,  // majority-rule resolved
    blue_shots: u64, blue_saves: u64, blue_assists: u64, blue_demos: u64,
    orange_team_id: Option<String>, // majority-rule resolved
    orange_shots: u64, orange_saves: u64, orange_assists: u64, orange_demos: u64,
}
```

The Kotlin mobile client queries a single path (`matches_index?orderBy="blue_team_id"`)
to render per-match team progression charts without joins or multiple reads.

See [ADR 0023](decisions/0023-denormalized-team-stats-in-matches-index.md) for the full design
rationale.

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

1. An immediate snapshot of the current `AppState` (connection status, last event type,
   lane routing statistics).
2. Subsequent pushes whenever `AppState` changes.

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
    "type": "Firebase",
    "url": "https://<project>.firebaseio.com",
    "apiKey": "<firebase-web-api-key>",
    "email": "<firebase-user-email>",
    "password": "<firebase-user-password>"
  },
  "reconnectDelaySeconds": 5,
  "uiSyncPort": 54321,
  "rememberPassword": false
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `isHeadless` | `bool` | `false` | Suppress UI Sync Server entirely |
| `websocketUrl` | `string` | `"ws://127.0.0.1:49123"` | Telemetry source endpoint |
| `connector.Firebase.url` | `string` | — | Firebase Realtime Database URL |
| `connector.Firebase.apiKey` | `string` | — | Firebase Web API key for REST auth |
| `connector.Firebase.email` | `string` | — | Firebase auth user email |
| `connector.Firebase.password` | `string` | — | Firebase auth user password |
| `reconnectDelaySeconds` | `u64` | `5` | Seconds before reconnection attempt |
| `uiSyncPort` | `u16` | `54321` | Port for the UI Sync Server |
| `rememberPassword` | `bool` | `false` | Persist password in config on save |
