#!/usr/bin/env bash
set -euo pipefail

if [ ! -d ".git" ]; then
  echo "error: must be run from the git repository root." >&2
  exit 2
fi

readonly SEARCH_ROOTS=(
  "core/src/daemon"
  "core/src/worker"
  "src-tauri/src/bridge"
)

readonly BLOCKED_PATTERN='(std::net::(TcpListener|TcpStream)|std::io::(BufReader|BufRead)|use\s+std::net::\{[^}]*\b(TcpListener|TcpStream)\b|use\s+std::io::\{[^}]*\b(BufReader|BufRead)\b)'
readonly EXCLUDED_FILE='core/src/daemon/client.rs'

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required for this check." >&2
  exit 2
fi

set +e
matches="$(rg --line-number --no-heading --glob "!${EXCLUDED_FILE}" --regexp "${BLOCKED_PATTERN}" "${SEARCH_ROOTS[@]}" 2>&1)"
rg_exit=$?
set -e

if [ "$rg_exit" -eq 2 ]; then
  echo "error: ripgrep failed (exit code 2):" >&2
  echo "${matches}" >&2
  exit 2
fi

if [ "$rg_exit" -eq 0 ]; then
  echo "error: found blocking std socket/buffer APIs in async runtime paths:" >&2
  echo "${matches}" >&2
  echo "hint: use tokio::net::{TcpListener, TcpStream} and tokio::io::{BufReader, AsyncBufReadExt, AsyncWriteExt}." >&2
  exit 1
fi

echo "ok: async runtime paths are free of blocked std socket/buffer APIs."
