#!/usr/bin/env bash
# prepare-sqlx.sh
#
# Run this once locally before merging the release workflow.
# Generates the .sqlx query cache so CI can build without a live database.
#
# Prerequisites:
#   - PostgreSQL running locally with volt DB (or use Docker Compose)
#   - cargo-sqlx installed: cargo install sqlx-cli --no-default-features --features postgres
#
# Usage:
#   docker compose up -d          # start local DB
#   chmod +x prepare-sqlx.sh
#   ./prepare-sqlx.sh
#   git add .sqlx
#   git commit -m "chore: add sqlx offline query cache"

set -euo pipefail

# Safer .env loading — handles spaces, special chars
if [ -f .env ]; then
  set -a
  source .env
  set +a
fi

: "${DATABASE_URL:=postgres://volt:volt@localhost:5432/volt}"

echo "→ Running migrations..."
cargo sqlx migrate run

echo "→ Generating .sqlx query cache..."
cargo sqlx prepare

echo ""
echo "✓ Done. Commit the .sqlx/ directory:"
echo ""
echo "  git add .sqlx"
echo "  git commit -m \"chore: add sqlx offline query cache\""
echo ""
echo "After that, tag a release to trigger the workflow:"
echo ""
echo "  git tag v0.1.0"
echo "  git push origin v0.1.0"
