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
plog="$proj/.git/sidetrack-proxy.log"
init_proj="$work/init_shop"
cleanup() {
  "$st" -C "$proj" proxy --stop >/dev/null 2>&1 || true
  "$st" -C "$work/wt" proxy --stop >/dev/null 2>&1 || true
  docker compose down -v --remove-orphans >/dev/null 2>&1 || true
  [ -d "$init_proj" ] && (cd "$init_proj" && docker compose down -v --remove-orphans >/dev/null 2>&1) || true
  rm -rf "$work"
}
trap cleanup EXIT

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { echo "FAIL: $*" >&2; [ -f "$plog" ] && cat "$plog" >&2; exit 1; }
# psql inside the app container, through the proxy, as the app would connect.
app_sql() { docker compose exec -T api sh -c 'psql "$DATABASE_URL" -qtAX -c "$0"' "$1"; }

step "init --proxy writes the config, with the app's URL written directly in compose"
# Alembic-shaped files are enough for detection; Alembic itself never runs here.
mkdir -p "$init_proj/migrations/versions"
cd "$init_proj"
printf '[alembic]\nscript_location = migrations\n' > alembic.ini
printf '# a1\n' > migrations/versions/a1_init.py
cat > compose.yml <<'EOF'
services:
  db:
    image: postgres:16-alpine
    environment: { POSTGRES_USER: app, POSTGRES_PASSWORD: secret, POSTGRES_DB: app_dev }
    healthcheck: { test: ["CMD", "pg_isready", "-U", "app"], interval: 1s, retries: 30 }
  api:
    image: postgres:16-alpine
    command: ["sleep", "infinity"]
    working_dir: /app
    volumes: ["./:/app"]
    environment:
      DATABASE_URL: postgresql://app:secret@db:5432/app_dev
    depends_on: { db: { condition: service_healthy } }
EOF
git init -q -b main
git config user.email e2e@example.com
git config user.name e2e
git add -A
git commit -qm "init project"
docker compose up -d --wait db >/dev/null
"$st" init --proxy --yes --no-hook | tee "$work/init.txt"
grep -q "DATABASE_URL (api) in the compose file  (fine in proxy mode)" "$work/init.txt" \
  || fail "detection should accept the URL in compose in proxy mode"
grep -q "Linked main → app_dev" "$work/init.txt" || fail "init should adopt app_dev"
grep -q '^mode = "proxy"$' sidetrack.toml || { cat sidetrack.toml; fail "init --proxy should write proxy mode"; }
grep -q '^database = "app_dev"$' sidetrack.toml || { cat sidetrack.toml; fail "routing.database should be app_dev"; }
grep -Eq '^restart = \[ ?"api" ?\]$' sidetrack.toml || { cat sidetrack.toml; fail "api should be the app service"; }
if grep -q '^file = ' sidetrack.toml; then cat sidetrack.toml; fail "there's no env file to record"; fi
docker compose down -v --remove-orphans >/dev/null 2>&1
cd "$proj"

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
file = ".env"              # worktrees get their own port written here
variable = "DATABASE_URL"
restart = ["api"]

[migrations]
tool = "generic"
dirs = ["migrations"]
command = "docker compose exec -T api sh /app/migrate.sh"
version_table = "schema_migrations"
version_column = "name"
EOF
printf '.env\n' > .gitignore
printf 'DATABASE_URL=postgresql://app:secret@host.docker.internal:54329/app_dev\n' > .env
git init -q -b main
git config user.email e2e@example.com
git config user.name e2e
git add -A
git commit -qm "proxy project"
docker compose up -d --wait >/dev/null
"$st" proxy --daemon | tee "$work/daemon.txt"
grep -q "Proxy running on 0.0.0.0:54329" "$work/daemon.txt" || fail "proxy --daemon should start it"
"$st" proxy --daemon | grep -q "already running" || fail "a second --daemon should notice the first"
grep -q "127.0.0.1:55433" "$plog" || fail "the proxy should find the published port"
docker compose exec -T api sh /app/migrate.sh >/dev/null
"$st" init --yes --no-hook
[ "$(app_sql 'select current_database()')" = app_dev ] || fail "app should start on app_dev"

step "a new branch: the app's app_dev now means feature_a_db"
git checkout -q -b feature/a
printf 'CREATE TABLE orders (id int);\n' > migrations/002_orders.sql
git add -A && git commit -qm orders
"$st" proxy --stop >/dev/null
"$st" switch --yes 2>&1 | tee "$work/a.txt"
grep -q "Proxy running on" "$work/a.txt" || fail "switch should start the proxy (autostart)"
grep -q "Route app_dev through the proxy to feature_a_db" "$work/a.txt" || fail "plan should route through the proxy"
grep -q "Hold new connections to app_dev" "$work/a.txt" || fail "copying app_dev should hold connections, not stop the app"
if grep -q "Stop api" "$work/a.txt"; then fail "the app shouldn't be stopped when the proxy can hold"; fi
[ "$(docker compose ps --services --status running | grep -c api)" = 1 ] || fail "api should still be running"
[ ! -e .git/sidetrack-proxy-hold ] || fail "the hold should be released"
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
grep -q "closed 1 connection" "$plog" || fail "proxy should log the closed connection"

step "sslmode=require and verify-full clients connect through the proxy over TLS"
tls_sql() { docker compose exec -T api sh -c 'psql "$DATABASE_URL?$0" -qtAX -c "select current_database()"' "$1"; }
[ "$(tls_sql sslmode=require)" = app_dev ] || fail "an sslmode=require client should connect through the proxy"
cp "$work/proxy-tls.pem" proxy-tls.pem   # the proxy's certificate, next to the registry
[ "$(tls_sql 'sslmode=verify-full&sslrootcert=/app/proxy-tls.pem')" = app_dev ] \
  || fail "the proxy's certificate should verify for host.docker.internal"
rm proxy-tls.pem

step "a git worktree gets its own database and its own proxy port"
wt="$work/wt"
git worktree add -q -b feature/w "$wt"
"$st" -C "$wt" switch --yes 2>&1 | tee "$work/wt.txt"
grep -q "Proxy running on 0.0.0.0:54330" "$work/wt.txt" || fail "the worktree's proxy should get the next port"
grep -q "Create feature_w_db" "$work/wt.txt" || fail "the worktree should get its own DB branch"
grep -q "Copy .env from" "$work/wt.txt" || fail "the worktree should get a copy of .env"
grep -q "^DATABASE_URL=postgresql://app:secret@host.docker.internal:54330/app_dev$" "$wt/.env" \
  || { cat "$wt/.env"; fail "the worktree's .env should point at its own proxy port"; }
wt_url="$(sed -n 's/^DATABASE_URL=//p' "$wt/.env")"
wt_sql() { docker compose exec -T api psql "$wt_url" -qtAX -c "$1"; }
[ "$(wt_sql 'select current_database()')" = feature_w_db ] || fail "the worktree's app_dev should mean feature_w_db"
[ "$(app_sql 'select current_database()')" = app_dev ] || fail "the main worktree should still be on app_dev"
"$st" -C "$wt" status | tee "$work/wt-status.txt"
grep -q "Worktree:" "$work/wt-status.txt" || fail "status should say it's a linked worktree"
grep -q "Elsewhere:   app_dev" "$work/wt-status.txt" || fail "status should show the main worktree's database"
if "$st" delete feature/w --yes 2>"$work/del.txt"; then fail "deleting a database another worktree uses should be refused"; fi
grep -q "in use in the worktree" "$work/del.txt" || fail "delete should say which worktree uses it"
"$st" -C "$wt" proxy --stop >/dev/null
git worktree remove --force "$wt"
if "$st" status | grep -q "Elsewhere:"; then fail "a removed worktree shouldn't count as using a database"; fi
"$st" delete feature/w --yes >/dev/null || fail "after the worktree is gone, its database can be deleted"

step "status and doctor know about the proxy"
"$st" status | tee "$work/status.txt"
grep -q "Proxy:       app_dev on 0.0.0.0:54329" "$work/status.txt" || fail "status should show the proxy"
"$st" doctor | tee "$work/doctor.txt" || true   # exits 1: no git hook here
grep -q "proxy upstream" "$work/doctor.txt" || fail "doctor should check the proxy"

"$st" proxy --stop | grep -q "Stopped the proxy" || fail "proxy --stop should stop it"
sleep 0.5
"$st" switch feature/a --yes --dry-run | tee "$work/down.txt"
grep -q "the proxy isn't running" "$work/down.txt" || fail "plans should warn when the proxy is down"

printf '\n\033[1;32mAll proxy end-to-end checks passed.\033[0m\n'
