#!/usr/bin/env bash
set -euo pipefail

DATABASE_URL="${DATABASE_URL:-postgres://volt:volt@localhost:5432/volt}"

echo "Bootstrapping Volt database at ${DATABASE_URL}"
if command -v psql >/dev/null 2>&1; then
  psql "${DATABASE_URL}" -f migrations/0001_core.sql
else
  echo "psql not found. Start Docker Compose and run: cargo run -- init-db"
fi