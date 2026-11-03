#!/bin/sh
# Tiny migration runner for the e2e test. Like Alembic or Flyway, it refuses to
# run when the database holds migrations this branch doesn't know about.
set -eu
cd "$(dirname "$0")"
q() { PGOPTIONS="-c client_min_messages=warning" psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -qtAX "$@"; }
q -c "CREATE TABLE IF NOT EXISTS schema_migrations (name text PRIMARY KEY)"
for applied in $(q -c "SELECT name FROM schema_migrations ORDER BY name"); do
  if [ ! -f "migrations/$applied" ]; then
    echo "migrate: database has $applied, which this branch doesn't know" >&2
    exit 1
  fi
done
for f in $(ls migrations | sort); do
  if [ -z "$(q -c "SELECT 1 FROM schema_migrations WHERE name = '$f'")" ]; then
    echo "migrate: applying $f"
    q -1 -f "migrations/$f" -c "INSERT INTO schema_migrations VALUES ('$f')"
  fi
done
echo "migrate: up to date"
