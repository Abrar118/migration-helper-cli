# sidetrack

**Give every git branch its own local database.**

You're on `feature/a` with 2 new migrations applied. You get pulled onto `feature/b`, which applies 4 more. Back on `feature/a`, Alembic (or Flyway, Django, Prisma, …) refuses to run: the database holds revisions this branch has never heard of. Usually the fix is checking out `feature/b` again and downgrading one migration at a time.

sidetrack removes that step. Each git branch gets a real database named after it (`feature/a` → `feature_a_db`). When you switch branches, sidetrack switches which database your app uses. Nothing is undone, so it works even with tools that have no down migrations.

```text
$ git checkout feature/a
Switched to branch 'feature/a'

Switch DB from feature_b_db to feature_a_db for feature/a
Plan:
   1. Point DATABASE_URL in .env at feature_a_db (was feature_b_db)
   2. Recreate api
Apply? [Y]es / [n]o / [a]lways for this repo
```

Status: **v2, Postgres only.** Routing by env file or by a local proxy. See [`docs/spec-v1.md`](docs/spec-v1.md) for the full spec and [`docs/plan-v1.md`](docs/plan-v1.md) for how it's built.

## Requirements

- Your database runs in **Docker Compose** (Postgres 13+; 15+ copies faster, 18 can copy-on-write).
- Your app reads its database name from an **untracked env file** (e.g. `DATABASE_URL` in `.env`).
- `git` and `docker compose` on your PATH. No Postgres client is needed on the host.

## Install

```bash
cargo install --path crates/sidetrack
```

## Quick start

From your project, with the database container running:

```bash
sidetrack init
```

`init` reads your compose setup and shows what it found. That's the database service and user, the env file and variable that hold the DB name, the migration tool, and the container it runs in. It then writes `sidetrack.toml` (safe to commit, no secrets), links the current branch to the current database, and offers to install the git hook.

After that, just use git. When you check out a branch, the hook shows a plan and asks before doing anything. Without a terminal (VS Code, other git GUIs) it prints a one-line hint instead of prompting:

```text
sidetrack: Create feature_c_db for feature/c and switch to it (from app_dev) — run `sidetrack switch` to apply
```

## How it works

- **Switching to a branch that has a DB branch:** rewrites the DB name in your env file, recreates the app containers, then runs any migrations the branch has that the database doesn't.
- **Switching to a branch for the first time:** copies the closest compatible database, then migrates forward. "Compatible" means it holds no migration the new branch doesn't know, judged from the migration files in git, so it works with any migration tool. It picks the copy that needs the fewest migrations run. If nothing is compatible, it builds from an empty database (and runs your seed command, if configured).
- **What's applied comes from your migration tool's own table** when it has one: `alembic_version` (following the `down_revision` chain), `django_migrations`, `flyway_schema_history`, Rails' `schema_migrations`, `_prisma_migrations` or `knex_migrations`. A database holding a migration no branch knows any more is never used as a starting point. For other tools, set `version_table` / `version_column`. Without a table, sidetrack falls back to its own records.
- **Copies** use Postgres template databases (`CREATE DATABASE … TEMPLATE`), so there's no dump and restore. Postgres needs nothing connected to the source while copying, so sidetrack stops the app services first when the source is the database in use.
- **Switch plans show what changes in the schema**, e.g. `schema vs feature_b_db: +1 table (orders) · −4 tables (…)`. Use `-v` for column-level changes.
- **Plans check disk space** in the database container first. One that won't fit is refused; one that would leave less than 1 GB (or 10%) free gets a warning.
- **Every change is a plan first.** `--dry-run` shows it, `--json` prints it for scripts, and if a step fails, completed steps are undone. The one exception is your own migration command failing: the new database then stays selected so you can debug it.
- **State** lives in a small SQLite registry (`~/.local/share/sidetrack/registry.sqlite`). The database server is treated as the truth, and mismatches are shown, never silently fixed.

## Commands

| Command | What it does |
|---|---|
| `sidetrack init` | Detect the setup, write `sidetrack.toml`, link the current branch to the current DB, offer the hook |
| `sidetrack status` | Current branch, active database, and what `switch` would do |
| `sidetrack list` | DB branches and snapshots with size, migration count and last use |
| `sidetrack switch [branch]` | Switch the database (default: the checked-out branch) |
| `sidetrack create <branch> [--from X \| --empty]` | Create a DB branch without switching to it |
| `sidetrack snapshot [label]` | Freeze a copy of the active database |
| `sidetrack snapshot list` / `snapshot rm <name>` | Manage snapshots |
| `sidetrack reset [--to <snapshot> \| --parent]` | Replace the active database with a fresh copy (default: latest snapshot, else parent) |
| `sidetrack delete <branch>` | Drop a DB branch and its snapshots |
| `sidetrack gc` | Drop DB branches whose git branch was deleted or merged |
| `sidetrack hook install` / `uninstall` | Manage the `post-checkout` hook |
| `sidetrack rename <old> <new>` | Follow `git branch -m`: move the DB branch to the new name |
| `sidetrack bench [--from X]` | Time a copy of a database here (then delete the copy) and estimate copy times for bigger ones |
| `sidetrack proxy [--listen ADDR]` | Run the local proxy (proxy mode) in the foreground |
| `sidetrack proxy --daemon` / `--stop` | Start the proxy in the background (log and pid in `.git/`) / stop it |
| `sidetrack doctor` | Check Docker, Postgres, disk space, the env file, the hook and more |

Global flags: `--dry-run`, `--json`, `--yes`, `-v` (show registry bookkeeping steps), `-C <dir>`.

Answering **[a]lways** applies future non-destructive plans in that repo without asking (stored on your machine, not in `sidetrack.toml`). Plans that drop data always ask. Set `SIDETRACK_SKIP=1` to skip the hook for one command.

## Configuration

`sidetrack init` writes this for you. Every field can be edited.

```toml
[database]
engine = "postgres"
service = "db"            # compose service running Postgres
user = "app"              # needs CREATEDB (the image's POSTGRES_USER is a superuser)
# psql = ["psql", "-h", "localhost", "-U", "app"]   # optional: run psql another way

[routing]
mode = "env"
file = ".env"             # must not be tracked by git
variable = "DATABASE_URL" # a URL (DB name = its path) or a bare DB name
restart = ["api", "worker"]

[migrations]
tool = "alembic"
dirs = ["alembic/versions"]                      # one file/folder per migration
command = "docker compose exec -T api alembic upgrade head"
# seed = "docker compose exec -T api python -m app.seed"   # after building from empty
# version_table = "schema_migrations"   # where your tool records applied migrations
# version_column = "name"               # (only needed for tools sidetrack doesn't know)

[git]
main_branch = "main"      # used by `gc` to find merged branches

[snapshots]
keep = 5                  # optional: `gc` drops older unlabelled snapshots per branch
```

### Proxy mode: no restarts

Instead of rewriting an env file and recreating the app, point the app at a local proxy once and leave it:

```toml
[routing]
mode = "proxy"
database = "app_dev"          # the name your app connects to
listen = "0.0.0.0:54320"      # default 127.0.0.1:54320
restart = ["api"]             # stopped only while its database is being copied
# upstream = "127.0.0.1:5432" # default: the port compose publishes for the db service
```

```yaml
# compose.yml
services:
  db:
    ports: ["5432:5432"]                                # the proxy reaches Postgres here
  api:
    environment:
      DATABASE_URL: postgresql://app:secret@host.docker.internal:54320/app_dev
    extra_hosts: ["host.docker.internal:host-gateway"]  # needed on Linux
```

- **Setting it up:** `sidetrack init --proxy` writes this config for you. It reads the app's database name from your env file, then tells you what to change in the app's URL (that's a one-time edit).
- **Running it:** sidetrack starts the proxy in the background when a command or the git hook needs it (`autostart = false` turns that off). You can also start it with `sidetrack proxy --daemon`, or run `sidetrack proxy` in its own terminal. Stop it with `sidetrack proxy --stop`.
- **No restarts, even for copies:** while the proxy is running, copying the database the app is using doesn't stop the app. The proxy holds the app's new connections for the few seconds the copy takes, then lets them through to the new database. A switch then only changes where `app_dev` goes. The proxy closes the app's open connections to the old database, so its connection pool reconnects to the new one.
- **Other databases:** connections to any other name pass through unchanged.
- **Encryption:** the proxy doesn't speak TLS. Use `sslmode=prefer` (the default) or `disable`.
- **Linux:** when the app runs in a container, listen on `0.0.0.0` (or the Docker bridge address), not `127.0.0.1`.

Detected migration tools: Alembic, Django, Prisma, Rails, Flyway and Knex. Anything else works too: point `dirs` at the migration folder(s) and set `command`.

## Limitations (v1)

- **Postgres only.** MySQL/MariaDB is next, then SQL Server and Oracle (see the spec's phasing).
- **Env-file routing recreates the app on each switch.** Proxy mode avoids that. While a copy of the app's database runs, the app's requests wait for the copy to finish.
- **Without a version table** (a tool sidetrack doesn't know, and no `version_table` set), migration state comes from file names in git and sidetrack's own records. If you run migrations yourself, those can lag behind. The only effect is that a later `switch` may run your migration command when there's nothing new to apply, which is harmless.
- **Every DB branch is a full copy** unless you're on Postgres 18 with a copy-on-write filesystem. Use `gc` and `delete` to reclaim space.
- **One database service per project.**

## Development

```bash
cargo test                       # unit tests; Postgres tests skip without a server
SIDETRACK_TEST_PG="docker exec -i <container> psql -U postgres" cargo test   # include them
tests/e2e/run.sh                 # full scenario in Docker (spec §1)
tests/e2e/alembic.sh             # the same with real Alembic (needs alembic + psycopg on PATH)
tests/e2e/proxy.sh               # proxy mode
```

Layout:

```text
crates/sidetrack-core   all logic: naming, config, registry, git, compose, detection,
                        Postgres engine, planner, applier
crates/sidetrack        the CLI and git hook
tests/e2e               a compose fixture and the end-to-end script
```
