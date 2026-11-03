#!/usr/bin/env bash
# End-to-end test: replays the scenario from docs/spec-v1.md §1 with real Docker.
#
#   main      : 001
#   feature/a : 001 + 2 migrations
#   feature/b : 001 + 4 migrations
#
# Switching back to feature/a after feature/b must leave a database that
# feature/a's migration tool accepts, without undoing anything by hand.
#
# Usage: tests/e2e/run.sh            (needs docker + compose, builds sidetrack)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
cargo build -q --manifest-path "$repo_root/Cargo.toml" -p sidetrack
st="$repo_root/target/debug/sidetrack"

work="$(mktemp -d)"
proj="$work/shop"
export SIDETRACK_REGISTRY="$work/registry.sqlite"
cp -r "$here/fixture" "$proj"
cd "$proj"

cleanup() {
  docker compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
sql() { docker compose exec -T db psql -U app -d "$1" -tAXc "$2"; }
active_db() { sed -n 's|^DATABASE_URL=.*/\([^/?]*\).*|\1|p' .env; }
migrate() { docker compose exec -T api sh /app/migrate.sh; }
commit_migration() { printf '%s\n' "$2" > "migrations/$1"; git add "migrations/$1"; git commit -qm "add $1"; }

step "set up repo and containers"
git init -q -b main
git config user.email e2e@example.com
git config user.name e2e
git add -A
git commit -qm "initial"
printf 'DATABASE_URL=postgresql://app:secret@db:5432/app_dev\n' > .env
docker compose up -d --wait >/dev/null
migrate

step "init (config is committed, like a teammate cloning the repo)"
"$st" init --yes
"$st" doctor

step "feature/a: new branch gets its own database"
git checkout -q -b feature/a
commit_migration 002_a_orders.sql "CREATE TABLE orders (id serial PRIMARY KEY);"
commit_migration 003_a_order_total.sql "ALTER TABLE orders ADD COLUMN total int;"
"$st" switch --yes
[ "$(active_db)" = feature_a_db ] || fail "expected feature_a_db, got $(active_db)"
[ "$(sql feature_a_db "SELECT count(*) FROM schema_migrations")" = 3 ] || fail "feature_a_db should have 3 migrations"
[ "$(sql app_dev "SELECT count(*) FROM schema_migrations")" = 1 ] || fail "app_dev must be untouched"

step "feature/b: 4 more migrations, based on main (not on feature/a)"
git checkout -q main
git checkout -q -b feature/b
for n in 1 2 3 4; do commit_migration "01${n}_b.sql" "CREATE TABLE b_table_$n (id int);"; done
"$st" switch --yes
[ "$(active_db)" = feature_b_db ] || fail "expected feature_b_db"
[ "$(sql feature_b_db "SELECT count(*) FROM schema_migrations")" = 5 ] || fail "feature_b_db should have 5 migrations"
[ "$(sql feature_b_db "SELECT count(*) FROM pg_tables WHERE tablename = 'orders'")" = 0 ] || fail "feature_b_db must not contain feature/a's tables"

step "the original problem: feature/a's code against feature/b's database"
git checkout -q feature/a 2>/dev/null   # the hook (no terminal here) only prints a hint
if migrate 2>/dev/null; then fail "migrate should refuse feature/b's database"; fi
echo "migrate refused, as expected"

step "back to feature/a: just a switch, nothing undone"
"$st" status || true
"$st" switch --yes | tee "$work/switch.txt"
[ "$(active_db)" = feature_a_db ] || fail "expected feature_a_db"
grep -q "schema vs feature_b_db: +1 table (orders) · −4 tables" "$work/switch.txt" || fail "switch should summarize the schema difference"
migrate | tail -1 | grep -q "up to date" || fail "feature/a's migrations should accept feature_a_db"

step "snapshot and reset"
"$st" snapshot before-experiment --yes
docker compose exec -T api sh -c 'psql "$DATABASE_URL" -qc "INSERT INTO orders (total) VALUES (42)"'
[ "$(sql feature_a_db "SELECT count(*) FROM orders")" = 1 ] || fail "row should be there"
"$st" reset --to before-experiment --yes
[ "$(sql feature_a_db "SELECT count(*) FROM orders")" = 0 ] || fail "reset should bring back the snapshot"
migrate | tail -1 | grep -q "up to date" || fail "after reset the DB should still match feature/a"

step "snapshot retention keeps labelled and newest snapshots"
"$st" snapshot --yes >/dev/null
"$st" snapshot --yes >/dev/null
printf '\n[snapshots]\nkeep = 1\n' >> sidetrack.toml
"$st" gc --yes
db_exists() { [ "$(sql postgres "SELECT count(*) FROM pg_database WHERE datname = '$1'")" = 1 ]; }
db_exists feature_a_db__s1 || fail "labelled snapshot must be kept"
db_exists feature_a_db__s3 || fail "newest snapshot must be kept"
if db_exists feature_a_db__s2; then fail "older unlabelled snapshot should be dropped"; fi
git checkout -q -- sidetrack.toml

step "bench times a copy and cleans up"
"$st" bench --from app_dev --yes | tee "$work/bench.txt"
grep -q "too small to estimate" "$work/bench.txt" || fail "bench should explain why it gives no estimate for a tiny database"
if db_exists app_dev__bench; then fail "bench must drop its copy"; fi

step "a branch forked from main after main moved on"
git checkout -q main
commit_migration 004_main_audit.sql "CREATE TABLE audit (id int);"
git checkout -q -b feature/c
"$st" switch --yes --dry-run | tee "$work/plan.txt"
grep -q "copy of app_dev" "$work/plan.txt" || fail "feature/c should start from app_dev"
"$st" switch --yes
[ "$(sql feature_c_db "SELECT count(*) FROM schema_migrations")" = 2 ] || fail "feature_c_db should have 001 + 004"

step "an older branch reuses a database that's behind its git branch"
# main now has 004 in git, but app_dev never ran it. Its version table says so,
# so a branch from before 004 can start from app_dev instead of from empty.
git checkout -q -b feature/old main~1
"$st" switch --yes --dry-run | tee "$work/plan_old.txt"
grep -q "copy of app_dev" "$work/plan_old.txt" || fail "feature/old should start from app_dev"
grep -q "per schema_migrations" "$work/plan_old.txt" || fail "the plan should say it read schema_migrations"
"$st" switch --yes >/dev/null
"$st" status | tee "$work/status_old.txt"
grep -q "Migrations:  1 applied in feature_old_db (per schema_migrations) · 0 pending · 0 unknown" "$work/status_old.txt" \
  || fail "status should report migrations read from the version table"

step "list"
"$st" list

step "gc after deleting feature/b"
git checkout -q feature/a
"$st" switch --yes >/dev/null
git branch -q -D feature/b
"$st" gc --yes
[ "$(sql postgres "SELECT count(*) FROM pg_database WHERE datname = 'feature_b_db'")" = 0 ] || fail "gc should drop feature_b_db"

step "a broken migration keeps the new database selected"
git checkout -q -b feature/d
commit_migration 020_broken.sql "THIS IS NOT SQL;"
if "$st" switch --yes; then fail "switch should report the failed migration"; fi
[ "$(active_db)" = feature_d_db ] || fail "feature_d_db should stay selected for debugging"
"$st" status | grep -q "out of sync" || fail "status should show the pending migration"

if command -v script >/dev/null 2>&1; then
  step "hook asks in a terminal and switches on yes"
  git checkout -q -f feature/a 2>/dev/null || true   # no terminal: prints a hint only
  git checkout -q -f feature/d
  # `script` gives the hook a pseudo-terminal; "y" answers the prompt.
  printf 'y\n' | script -qec "git checkout feature/a" /dev/null | tee "$work/hook.txt"
  grep -q "Apply?" "$work/hook.txt" || fail "the hook should have asked"
  [ "$(active_db)" = feature_a_db ] || fail "the hook should have switched to feature_a_db"
fi

step "rename follows git branch -m"
git branch -m feature/c feature/renamed
"$st" rename feature/c feature/renamed --yes
db_exists feature_renamed_db || fail "feature_c_db should now be feature_renamed_db"
if db_exists feature_c_db; then fail "feature_c_db should be gone"; fi
"$st" list | grep -q "feature/renamed" || fail "list should show the renamed branch"
"$st" doctor | grep -q "disk space" || fail "doctor should report disk space"

printf '\n\033[1;32mAll end-to-end checks passed.\033[0m\n'
