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
│   │   ├── models.rs           MatchIndexEntry, PlayerMatchLog, Cumulative*Stats, PlayerRegistryEntry
│   │   ├── firebase.rs         FirebaseConnector (HTTP sink)
│   │   ├── firebase_auth.rs    Firebase REST authentication (token exchange)
│   │   ├── daemon/
│   │   │   ├── mod.rs          DaemonSupervisor, run_daemon orchestration
│   │   │   ├── control.rs      IPC Control Server (TCP listener)
│   │   │   ├── client.rs       Synchronous IPC client for CLI invocation
│   │   │   ├── protocol.rs     ControlCommand / ControlReply enums
│   │   │   └── ui_server.rs    WebSocket UI Sync Server
│   │   └── worker/
│   │       ├── mod.rs           Ingestion Engine (session loop, reconnection, compaction, routing)
│   │       ├── actors.rs        Sink actor tasks + retry/backoff + live-state coalescing
│   │       ├── aggregation.rs   Match finalization, majority-rule roster resolution, cumulative stats
│   │       ├── events.rs        RocketLeagueEvent, IngestClass, IngestEnvelope
│   │       ├── context.rs       SessionContext (match/session ID tracking, replay detection)
│   │       └── transformer.rs   Payload normalization (camelCase/snake_case) + stat extraction
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
├── scripts/
│   └── check-no-sync-socket-async.sh   CI guardrail: forbid blocking socket APIs in async paths
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

### Blocking Socket Guardrail (`check-no-sync-socket-async.sh`)

This script prevents accidental introduction of synchronous blocking I/O inside async
runtime paths. It scans the following directories with `ripgrep`:

| Search Root | Purpose |
|-------------|---------|
| `core/src/daemon` | Daemon supervisor, control server |
| `core/src/worker` | Ingestion engine, sink actors, aggregation |
| `src-tauri/src/bridge` | Tauri IPC transport and UI sync |

**Blocked patterns:**
- `std::net::TcpListener` / `std::net::TcpStream` (use `tokio::net` equivalents)
- `std::io::BufReader` / `std::io::BufRead` (use `tokio::io` equivalents)
- Any `use std::net::{...}` or `use std::io::{...}` import blocks containing those types

**Exclusion:** `core/src/daemon/client.rs` is exempt — it is an intentionally synchronous
IPC client for short-lived CLI processes that do not share the async runtime.

The script uses exit codes for CI integration:

| Exit Code | Meaning |
|-----------|---------|
| 0 | Clean: no blocking APIs found |
| 1 | Violation found: blocking APIs detected; CI must fail |
| 2 | Tooling error: missing `rg` or repository root check |

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
