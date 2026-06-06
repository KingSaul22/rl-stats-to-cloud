#!/usr/bin/env bash
set -euo pipefail

readonly SEARCH_ROOTS=(
  "core/src/daemon"
  "core/src/worker"
  "src-tauri/src/bridge"
)

readonly BLOCKED_PATTERN='std::net::TcpListener|std::net::TcpStream|std::io::BufReader|std::io::BufRead'
readonly EXCLUDED_FILE='core/src/daemon/client.rs'

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required for this check." >&2
  exit 2
fi

if matches="$(rg --line-number --no-heading --glob "!${EXCLUDED_FILE}" --regexp "${BLOCKED_PATTERN}" "${SEARCH_ROOTS[@]}")"; then
  echo "error: found blocking std socket/buffer APIs in async runtime paths:" >&2
  echo "${matches}" >&2
  echo "hint: use tokio::net::{TcpListener, TcpStream} and tokio::io::{BufReader, AsyncBufReadExt, AsyncWriteExt}." >&2
  exit 1
fi

echo "ok: async runtime paths are free of blocked std socket/buffer APIs."
