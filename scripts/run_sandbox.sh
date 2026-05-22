#!/usr/bin/env bash
set -euo pipefail

# Hardened Linux helper for untrusted package tests.
# Usage: ./scripts/run_sandbox.sh 'python3 tool.py'
# Requires Linux namespaces and unshare.

COMMAND="${1:?missing command}"
TIMEOUT_SECONDS="${VOLT_SANDBOX_TIMEOUT_SECONDS:-5}"
MEMORY_KB="${VOLT_SANDBOX_MEMORY_KB:-65536}"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT

if command -v unshare >/dev/null 2>&1; then
  if command -v prlimit >/dev/null 2>&1; then
    unshare --user --map-root-user --net --pid --fork --mount-proc \
      prlimit --as="${MEMORY_KB}"000 --cpu="${TIMEOUT_SECONDS}" \
      timeout "${TIMEOUT_SECONDS}" bash -lc "cd '${WORKDIR}' && ${COMMAND}"
  else
    unshare --user --map-root-user --net --pid --fork --mount-proc \
      timeout "${TIMEOUT_SECONDS}" bash -lc "cd '${WORKDIR}' && ${COMMAND}"
  fi
else
  echo "unshare not available; falling back to timeout-only execution" >&2
  timeout "${TIMEOUT_SECONDS}" bash -lc "cd '${WORKDIR}' && ${COMMAND}"
fi