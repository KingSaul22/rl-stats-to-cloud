# Tauri + Vanilla TS

This template should help get you started developing with Tauri in vanilla HTML, CSS and Typescript.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Execution
For development with the GUI:
```
bun run tauri dev
```

For headless execution:
```
cargo run --manifest-path src-tauri/Cargo.toml -- --headless
```


```
cargo clippy --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
```

```
cargo outdated && cargo upgrade && cargo update && cargo check
```