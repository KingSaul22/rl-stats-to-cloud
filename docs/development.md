# Development

## Prerequisites

- **Rust** (edition 2024 toolchain)
- **Bun** (package manager and script runner for the frontend)
- **Tauri CLI** (installed via `npm`/`bun`, invoked through `bun run tauri`)

## Project Structure

```
.
├── core/                       rl_stats_core (Core Service)
│   ├── src/
│   │   ├── main.rs             Binary entrypoint + CLI arg parsing
│   │   ├── lib.rs              Public API re-exports
│   │   ├── config.rs           AppConfig, ConfigManager, config path
│   │   ├── connector.rs        EventSink trait, NullSink, connector factory
│   │   ├── firebase.rs         FirebaseConnector (HTTP sink)
│   │   ├── daemon/
│   │   │   ├── mod.rs          DaemonSupervisor, run_daemon orchestration
│   │   │   ├── control.rs      IPC Control Server (TCP listener)
│   │   │   ├── client.rs       Synchronous IPC client for CLI invocation
│   │   │   ├── protocol.rs     ControlCommand / ControlReply enums
│   │   │   └── ui_server.rs    WebSocket UI Sync Server
│   │   └── worker/
│   │       ├── mod.rs           Ingestion Engine (session loop, reconnection)
│   │       ├── actors.rs        Sink actor tasks + retry/backoff
│   │       ├── events.rs        RocketLeagueEvent, IngestEnvelope
│   │       ├── context.rs       SessionContext (match/session ID tracking)
│   │       └── transformer.rs   Payload normalization (camelCase/snake_case)
│   └── Cargo.toml
├── src-tauri/                   rl-stats-to-cloud (Tauri client)
│   ├── src/
│   │   ├── main.rs              Binary entry (config load, run_tauri)
│   │   ├── lib.rs               Library re-exports
│   │   ├── commands/mod.rs      Tauri commands (get_config, save_config, get_status)
│   │   └── bridge/
│   │       ├── mod.rs           run_tauri, shared state, shutdown hooks
│   │       ├── transport.rs     IPC client + UI Sync WebSocket loop
│   │       └── lifecycle.rs     spawn/shutdown UI bridge task
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   └── Cargo.toml
├── src/                         Frontend (TypeScript SPA)
│   ├── main.ts                   Entrypoint, event listeners, init logic
│   ├── api.ts                    Tauri invoke wrapper with Zod parsing
│   ├── ui.ts                     DOM rendering, form management
│   ├── schemas.ts                Zod schemas + normalization functions
│   ├── constants.ts              Command names, UI constants
│   └── styles.css                Application styles (dark theme)
├── index.html                    SPA shell
├── vite.config.ts                Vite 6 dev server config
├── tsconfig.json                 TypeScript 6 strict config
├── package.json                  Frontend dependencies
├── bun.lock
├── Cargo.toml                    Workspace root (members: core, src-tauri)
└── Cargo.lock
```

## Validation Commands

Run all checks before committing:

```bash
# Rust type-check (workspace)
cargo check --workspace

# Guardrail: forbid blocking std socket APIs in async runtime paths
bash scripts/check-no-sync-socket-async.sh

# Strict clippy: all + pedantic = deny, nursery = warn
cargo clippy --all-targets --all-features -- \
  -D warnings \
  -D clippy::all \
  -D clippy::pedantic \
  -D clippy::nursery

# Security advisory audit (must run from core/)
cargo deny check advisories

# Frontend type-check
bun run tsc

# Frontend build (production)
bun run build
```

## Linting Policy

### Crate-Level Lints

Both `core/` and `src-tauri/` enforce:

```toml
# Cargo.toml
[lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
todo = "deny"
panic = "deny"
all = "deny"
pedantic = "deny"
nursery = "warn"
```

### Clippy Thresholds

**`core/clippy.toml`** and **`src-tauri/clippy.toml`:**

```toml
cognitive-complexity-threshold = 10
too-many-arguments-threshold = 4
type-complexity-threshold = 50
```

### Rustfmt

**`src-tauri/rustfmt.toml`:**

```toml
max_width = 100
fn_call_width = 60
format_strings = true
reorder_imports = true
```

## Tauri Configuration

### `tauri.conf.json`

- Dev URL: `http://localhost:1420` (Vite strict port)
- CSP: `null` (disabled for development flexibility)
- Frontend dist: `../dist`
- Build: `bun run build` (TypeScript compilation + Vite production bundle)
- Dev command: `bun run dev`

### Capabilities

Minimal permissions: `core:default` and `opener:default`. The Tauri app does not require
filesystem, network, or shell access — all external communication flows through the Core Service.

## Dependency Updates

```bash
cargo outdated && cargo upgrade && cargo update && cargo check
```

## Firebase Schema Documentation

Reference schemas for the Firebase Realtime Database structure:

- `docs/firebase-base-schema.json` — v1 schema (nested `metadata`, `teams`, `players`, `calendar`)
- `docs/firebase-base-schema_v2.json` — v2 schema (flattened: `live_state`, `live_events_feed`,
  `matches_events_history`, `stats_cumulative`, `player_telemetry`)
