#!/usr/bin/env bash
# End-to-end test with real Alembic: the original "Can't locate revision" problem,
# and sidetrack reading alembic_version (heads + the down_revision chain).
#
# Alembic runs on this machine against the Postgres container (published on
# port 55432). Needs: docker compose, and `alembic` + `psycopg` on PATH
# (e.g. `pip install alembic "psycopg[binary]"`).
#
#   main      : a1
#   feature/a : a1 → b2
#   feature/b : a1 → c3 → d4
set -euo pipefail

command -v alembic >/dev/null || { echo "alembic isn't on PATH; skipping"; exit 0; }
here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
cargo build -q --manifest-path "$repo_root/Cargo.toml" -p sidetrack
st="$repo_root/target/debug/sidetrack"

work="$(mktemp -d)"
proj="$work/alembic_shop"
mkdir -p "$proj"
export SIDETRACK_REGISTRY="$work/registry.sqlite"
cd "$proj"
cleanup() {
  docker compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
migrate() { sh -c 'set -a; . ./.env; set +a; alembic upgrade head'; }
current() { sh -c 'set -a; . ./.env; set +a; alembic current 2>/dev/null' | awk '{print $1}' | sort | tr '\n' ' ' | sed 's/ $//'; }
# A real `alembic revision` file whose upgrade runs one SQL statement.
revision() { # id down message sql
  alembic revision --rev-id "$1" ${2:+--head "$2"} -m "$3" >/dev/null
  f=$(ls migrations/versions/"$1"_*.py)
  sed -i "0,/^    pass$/s||    op.execute(\"$4\")|" "$f"
  git add "$f"
  git commit -qm "alembic $1: $3"
}

step "set up an Alembic project with Postgres in Docker"
cat > compose.yml <<'EOF'
services:
  db:
    image: postgres:16-alpine
    environment: { POSTGRES_USER: app, POSTGRES_PASSWORD: secret, POSTGRES_DB: app_dev }
    ports: ["55432:5432"]
    healthcheck: { test: ["CMD", "pg_isready", "-U", "app"], interval: 1s, retries: 30 }
  api:
    image: postgres:16-alpine
    command: ["sleep", "infinity"]
    env_file: [.env]
    depends_on: { db: { condition: service_healthy } }
EOF
cat > sidetrack.toml <<'EOF'
[database]
engine = "postgres"
service = "db"
user = "app"

[routing]
mode = "env"
file = ".env"
variable = "DATABASE_URL"
restart = ["api"]

[migrations]
tool = "alembic"
dirs = ["migrations/versions"]
command = "sh -c 'set -a; . ./.env; set +a; alembic upgrade head'"
EOF
printf '.env\n__pycache__/\n' > .gitignore
alembic init -q migrations >/dev/null 2>&1 || alembic init migrations >/dev/null
# env.py: take the URL from DATABASE_URL.
sed -i 's|^config = context.config$|config = context.config\nimport os\nconfig.set_main_option("sqlalchemy.url", os.environ["DATABASE_URL"])|' migrations/env.py
printf 'DATABASE_URL=postgresql+psycopg://app:secret@localhost:55432/app_dev\n' > .env
git init -q -b main
git config user.email e2e@example.com
git config user.name e2e
git add -A
git commit -qm "alembic project"
revision a1 "" "users" "CREATE TABLE users (id int)"
docker compose up -d --wait >/dev/null
migrate
"$st" init --yes --no-hook
[ "$(current)" = a1 ] || fail "app_dev should be at a1"

step "feature/a and feature/b each get their own database"
git checkout -q -b feature/a
revision b2 a1 "orders" "CREATE TABLE orders (id int)"
"$st" switch --yes >/dev/null
[ "$(current)" = b2 ] || fail "feature_a_db should be at b2, got $(current)"
git checkout -q main
git checkout -q -b feature/b
revision c3 a1 "payments" "CREATE TABLE payments (id int)"
revision d4 c3 "refunds" "CREATE TABLE refunds (id int)"
"$st" switch --yes >/dev/null
[ "$(current)" = d4 ] || fail "feature_b_db should be at d4, got $(current)"

step "the original problem: Alembic on feature/a can't find feature/b's revision"
git checkout -q feature/a
if migrate 2>"$work/err.txt"; then fail "alembic should fail against feature/b's database"; fi
grep -q "Can't locate revision identified by 'd4'" "$work/err.txt" || { cat "$work/err.txt"; fail "expected Alembic's error"; }
echo "Alembic: $(grep -o "Can't locate revision identified by 'd4'" "$work/err.txt")"

step "sidetrack switches back and reads alembic_version"
"$st" switch --yes | tee "$work/switch.txt"
[ "$(current)" = b2 ] || fail "feature_a_db should be at b2"
migrate >/dev/null || fail "alembic should accept feature_a_db"
"$st" status | tee "$work/status.txt"
grep -q "Migrations:  2 applied in feature_a_db (per alembic_version) · 0 pending · 0 unknown" "$work/status.txt" \
  || fail "status should read 2 applied revisions (b2 and its parent a1) from alembic_version"

step "a branch from an older main reuses app_dev, which is behind main"
git checkout -q main
revision e5 a1 "audit" "CREATE TABLE audit (id int)"   # committed, never applied to app_dev
git checkout -q -b feature/old main~1
"$st" switch --yes | tee "$work/old.txt"
grep -q "copy of app_dev" "$work/old.txt" || fail "feature/old should start from app_dev"
grep -q "1 of 1 migrations applied per alembic_version" "$work/old.txt" || fail "base choice should come from alembic_version"
[ "$(current)" = a1 ] || fail "feature_old_db should be at a1"

step "a database whose revision no branch knows is never used as a base"
git branch -q -D feature/b   # c3 and d4 now exist only in feature_b_db
git checkout -q main
git checkout -q -b feature/new
"$st" switch --yes --dry-run | tee "$work/new.txt"
if grep -q "copy of feature_b_db" "$work/new.txt"; then fail "feature_b_db holds unknown revisions"; fi
"$st" status --json | grep -q '"reader_error":null' || fail "reading alembic_version should not have failed"

printf '\n\033[1;32mAll Alembic end-to-end checks passed.\033[0m\n'
