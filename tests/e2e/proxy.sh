#!/usr/bin/env bash
# End-to-end test for routing.mode = "proxy": the app always connects to app_dev
# through `sidetrack proxy`, and switching branches changes where that goes,
# with no env file and no container restart.
#
# The app container reaches the proxy on the host via host.docker.internal.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
cargo build -q --manifest-path "$repo_root/Cargo.toml" -p sidetrack
st="$repo_root/target/debug/sidetrack"

work="$(mktemp -d)"
proj="$work/proxy_shop"
mkdir -p "$proj"
export SIDETRACK_REGISTRY="$work/registry.sqlite"
cd "$proj"
proxy_pid=""
cleanup() {
  [ -n "$proxy_pid" ] && kill "$proxy_pid" 2>/dev/null || true
  docker compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { echo "FAIL: $*" >&2; [ -f "$work/proxy.log" ] && cat "$work/proxy.log" >&2; exit 1; }
# psql inside the app container, through the proxy, as the app would connect.
app_sql() { docker compose exec -T api sh -c 'psql "$DATABASE_URL" -qtAX -c "$0"' "$1"; }

step "set up a project whose app connects through the proxy"
cp "$here/fixture/migrate.sh" .
mkdir migrations
cp "$here/fixture/migrations/001_users.sql" migrations/
cat > compose.yml <<'EOF'
services:
  db:
    image: postgres:16-alpine
    environment: { POSTGRES_USER: app, POSTGRES_PASSWORD: secret, POSTGRES_DB: app_dev }
    ports: ["55433:5432"]
    healthcheck: { test: ["CMD", "pg_isready", "-U", "app"], interval: 1s, retries: 30 }
  api:
    image: postgres:16-alpine
    command: ["sleep", "infinity"]
    working_dir: /app
    volumes: ["./:/app"]
    environment:
      DATABASE_URL: postgresql://app:secret@host.docker.internal:54329/app_dev
    extra_hosts: ["host.docker.internal:host-gateway"]
    depends_on: { db: { condition: service_healthy } }
EOF
cat > sidetrack.toml <<'EOF'
[database]
engine = "postgres"
service = "db"
user = "app"

[routing]
mode = "proxy"
database = "app_dev"
listen = "0.0.0.0:54329"
restart = ["api"]

[migrations]
tool = "generic"
dirs = ["migrations"]
command = "docker compose exec -T api sh /app/migrate.sh"
version_table = "schema_migrations"
version_column = "name"
EOF
git init -q -b main
git config user.email e2e@example.com
git config user.name e2e
git add -A
git commit -qm "proxy project"
docker compose up -d --wait >/dev/null
"$st" proxy >"$work/proxy.log" 2>&1 &
proxy_pid=$!
for _ in $(seq 1 50); do grep -q "sidetrack proxy: 0.0.0.0:54329" "$work/proxy.log" && break; sleep 0.1; done
grep -q "127.0.0.1:55433" "$work/proxy.log" || fail "the proxy should find the published port"
docker compose exec -T api sh /app/migrate.sh >/dev/null
"$st" init --yes --no-hook
[ "$(app_sql 'select current_database()')" = app_dev ] || fail "app should start on app_dev"

step "a new branch: the app's app_dev now means feature_a_db"
git checkout -q -b feature/a
printf 'CREATE TABLE orders (id int);\n' > migrations/002_orders.sql
git add -A && git commit -qm orders
"$st" switch --yes | tee "$work/a.txt"
grep -q "Route app_dev through the proxy to feature_a_db" "$work/a.txt" || fail "plan should route through the proxy"
[ "$(app_sql 'select current_database()')" = feature_a_db ] || fail "app should be on feature_a_db"
[ "$(app_sql "select count(*) from information_schema.tables where table_name = 'orders'")" = 1 ] \
  || fail "orders should exist in feature_a_db"

step "switching back needs no restart, and open app connections move over"
git checkout -q main
docker compose exec -T api sh -c 'psql "$DATABASE_URL" -c "select pg_sleep(60)"' >/dev/null 2>&1 &
held=$!
sleep 1
"$st" switch --yes | tee "$work/main.txt"
if grep -q "Recreate" "$work/main.txt"; then fail "proxy mode shouldn't recreate the app"; fi
[ "$(app_sql 'select current_database()')" = app_dev ] || fail "app should be back on app_dev"
for _ in $(seq 1 30); do kill -0 "$held" 2>/dev/null || break; sleep 0.2; done
if kill -0 "$held" 2>/dev/null; then fail "the open connection to feature_a_db should have been closed"; fi
grep -q "closed 1 connection" "$work/proxy.log" || fail "proxy should log the closed connection"

step "status and doctor know about the proxy"
"$st" status | tee "$work/status.txt"
grep -q "Proxy:       app_dev on 0.0.0.0:54329" "$work/status.txt" || fail "status should show the proxy"
"$st" doctor | tee "$work/doctor.txt" || true   # exits 1: no git hook here
grep -q "proxy upstream" "$work/doctor.txt" || fail "doctor should check the proxy"

kill "$proxy_pid"; wait "$proxy_pid" 2>/dev/null || true; proxy_pid=""
"$st" switch feature/a --yes --dry-run | tee "$work/down.txt"
grep -q "the proxy isn't running" "$work/down.txt" || fail "plans should warn when the proxy is down"

printf '\n\033[1;32mAll proxy end-to-end checks passed.\033[0m\n'
