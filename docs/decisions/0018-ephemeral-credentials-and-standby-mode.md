# ADR 0018: Ephemeral Credentials and Standby Mode

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0018 |
| **Title** | Ephemeral Credentials and Standby Mode |
| **Status** | Accepted |
| **Date** | 2026-06-04 |

## Context
The daemon authenticates against Firebase using an email/password credential pair. In the original implementation, these credentials were stored as plaintext inside `%APPDATA%/config.json` and persisted to disk on every configuration save. This creates a persistent credential exposure surface: any process or user with filesystem read access to the config directory can extract the Firebase password. Additionally, the daemon would crash on startup if the config lacked a password field — there was no graceful degradation path for deployments where the operator prefers to supply credentials at runtime only.

A decision was required to eliminate the persistent credential footprint on disk while keeping the daemon operational in the absence of pre-configured credentials and providing a secure injection path for runtime credential supply.

## Decision
We implement **Ephemeral Credentials** with a three-part design:

### 1. Optional Password Field with Standby Mode
The `password` field in `config.json` is now optional. On startup, if the daemon finds no password configured, it enters a `MissingCredentials` standby state (`AuthRuntimeState::MissingCredentials`). The daemon boots successfully but does not attempt Firebase authentication or telemetry upload. This prevents crash-on-startup and decouples daemon availability from credential availability.

### 2. IPC `ProvidePassword` Control Command
The interprocess control protocol exposes a new `ControlCommand::ProvidePassword(String)` variant. The Tauri UI frontend can invoke this command over the local IPC channel (Windows named pipe / Unix socket) to inject a password directly into the daemon's runtime. On receipt, the daemon writes the password into the `Arc<RwLock<Option<String>>>` field on `FirebaseAuth`, resets the `TokenState` to default (clearing any stale tokens), transitions the runtime state from `MissingCredentials` to `Unauthenticated`, and immediately initiates Firebase authentication to obtain an ID token.

The password never touches the filesystem in this flow. It exists only in the daemon's protected heap memory (`RwLock<TokenState>`) for the lifetime of the process.

### 3. Opt-in Persistence with `remember_password` Flag
The configuration schema includes a `remember_password` boolean flag (default `false`). When the Tauri UI persists a config update to disk, it inspects this flag:

- If `remember_password` is `false`, the UI sanitizes the config payload by setting `password` to `None` *before* serializing to `config.json`. The password is forwarded to the daemon via `ProvidePassword` but is not written to disk.
- If `remember_password` is `true` (explicit user opt-in), the password is written to disk alongside other config fields, restoring the original behaviour.

This preserves user choice while defaulting to the secure ephemeral path.

## Rejected Alternatives
- **Config-file encryption at rest:** Rejected because encryption keys must themselves be stored or derived, creating a bootstrapping problem. A key stored on the same machine offers no meaningful increase in security over plaintext.
- **OS keychain integration:** Rejected for initial implementation due to platform-specific API surface (`windows-rs` credential manager vs `security` framework on macOS vs `libsecret` on Linux) and the complexity of consistent Tauri bindings across targets. Keychain integration remains a candidate for a future ADR.
- **Mandatory password in config with crash-on-missing:** Rejected because it forces the daemon offline in any deployment where the operator prefers runtime-only credential supply, reducing operational flexibility.

## Consequences

### Positive
- **Reduced credential exposure surface:** In the default configuration, the Firebase password never touches the filesystem. A compromised `config.json` reveals only the API key and email — insufficient for authentication.
- **Graceful degradation:** The daemon boots and remains reachable via IPC even when credentials are absent, enabling deferred credential injection through the UI.
- **Explicit user intent:** The `remember_password` flag makes persistence an explicit opt-in, avoiding accidental credential leakage from well-intentioned config saves.
- **Minimal attack window:** The password lives in guarded heap memory (`Arc<RwLock<Option<String>>>`) and is dropped on daemon shutdown, limiting exposure to process-memory forensics only.

### Negative / Limitations
- **User friction:** The operator must supply the password on every UI launch when `remember_password` is disabled, which may be frequent in development or unstable-host environments.
- **No cross-session token persistence:** The daemon starts cold on every boot. Even if a valid refresh token existed in the previous session, it is discarded. Every restart requires a fresh password injection and full authentication round-trip.
- **IPC dependency for credential supply:** If the IPC transport is unavailable or the UI crashes before forwarding the password, the daemon remains in `MissingCredentials` indefinitely with no autonomous recovery path until the next IPC connection.
- **No multi-factor authentication support:** The `ProvidePassword` flow only supports email/password authentication. Firebase providers requiring OAuth, phone, or TOTP are not accommodated by the current design.

### Mitigations
- The `remember_password` flag provides a user-controlled escape hatch for environments where convenience outweighs credential secrecy.
- Future work may introduce OS keychain integration (deferred to a separate ADR) to provide secure, persistent credential storage without plaintext files.
