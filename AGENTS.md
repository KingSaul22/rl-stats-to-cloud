# AGENTS.md

## Toolchain
- **Bun** (not npm) for JS/TS: `bun run tauri dev`, `bun run build`
- **Rust edition 2024** — very new, not default anywhere yet
- Vite dev server locked to port 1420 (`strictPort: true` in vite.config.ts)

## Workspace
Cargo workspace: `rl_stats_core` (core/) and `rl-stats-to-cloud` (src-tauri/).
Use `cargo run -p <package>` to target a specific package.

## Strict Linting
Both crates enforce `unsafe_code = "forbid"` and deny `unwrap_used`, `expect_used`, `todo`, `panic`.
Clippy `all` + `pedantic` = deny, `nursery` = warn.

```bash
cargo clippy --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
```

## Daemon Commands
`rl_stats_core` accepts CLI flags for control:
- (no flag) — start daemon
- `--allow-ui` — start UI WebSocket server at `ws://127.0.0.1:{ui_sync_port}`
- `--disallow-ui` — stop UI WebSocket server
- `--poweroff` — graceful shutdown

## Architecture
- Daemon and Tauri UI communicate via WebSocket (`ws://127.0.0.1:{ui_sync_port}` from `config.json`)
- Tauri UI depends on `rl_stats_core` as a library; sends `AllowUi` command to daemon on startup
- Interprocess communication uses the `interprocess` crate (Unix sockets / Windows named pipes)

## Tauri Quirks
- `src-tauri` lib name is `rl_stats_to_cloud_lib` (Windows Cargo bug workaround, see src-tauri/Cargo.toml:14)
- CSP disabled (`"csp": null` in tauri.conf.json)
- `beforeDevCommand`: `bun run dev`, `beforeBuildCommand`: `bun run build`
