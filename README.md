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

For a non-default Compose file, use `sidetrack init -f docker-compose-dev.yml` (repeat `-f` for overrides, in order). The selected files are saved in `[compose].files` and included in detected Compose migration commands.

After that, just use git. When you check out a branch, the hook shows a plan and asks before doing anything. Without a terminal (VS Code, other git GUIs) it prints a one-line hint instead of prompting:

```text
sidetrack: Create feature_c_db for feature/c and switch to it (from app_dev) — run `sidetrack switch` to apply
```

If the desktop app is open, the hook also sends it the checkout. The app then shows the plan for you to approve.

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
# file = ".env"               # optional: git worktrees set their proxy port in this URL
# variable = "DATABASE_URL"
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

- **Setting it up:** `sidetrack init --proxy` writes this config for you. It finds the app's database name in your env file, or in compose `environment:` (unlike env mode, proxy mode never has to change it there), then tells you what to change in the app's URL (that's a one-time edit). Git worktrees only get their own database when the URL is in an env file (see below).
- **Running it:** sidetrack starts the proxy in the background when a command or the git hook needs it (`autostart = false` turns that off). You can also start it with `sidetrack proxy --daemon`, or run `sidetrack proxy` in its own terminal. Stop it with `sidetrack proxy --stop`.
- **No restarts, even for copies:** while the proxy is running, copying the database the app is using doesn't stop the app. The proxy holds the app's new connections for the few seconds the copy takes, then lets them through to the new database. A switch then only changes where `app_dev` goes. The proxy closes the app's open connections to the old database, so its connection pool reconnects to the new one.
- **Other databases:** connections to any other name pass through unchanged.
- **Encryption:** the proxy accepts TLS, so `sslmode=prefer` (the default) and `require` both work. It uses a self-signed certificate that sidetrack creates once per machine, next to its registry (`proxy-tls.pem`, e.g. `~/.local/share/sidetrack/` on Linux). For `verify-full`, point `sslrootcert` at that file; it is valid for `localhost`, `127.0.0.1` and `host.docker.internal`. The proxy talks to Postgres without TLS.
- **Linux:** when the app runs in a container, listen on `0.0.0.0` (or the Docker bridge address), not `127.0.0.1`.

### Git worktrees

With proxy mode, each `git worktree` gets its own DB branch, so several branches can run side by side:

```bash
git worktree add -b feature/x ../shop-x
cd ../shop-x && sidetrack switch     # creates feature_x_db and starts this worktree's proxy
sidetrack status                     # shows its port (e.g. 54321) and what other worktrees use
```

- **Shared server:** every worktree uses the main worktree's database server. Start only the app services in a worktree, e.g. `docker compose up -d --no-deps api`.
- **Ports:** each linked worktree's proxy listens on the next free port after `routing.listen`, and keeps that port. With `file` and `variable` set under `[routing]` (`init --proxy` keeps them), the first `switch` in a worktree points the app's URL at that port. If the worktree has no env file yet, it copies the main worktree's first.
- **Removing a worktree** (`git worktree remove`) frees its port, and its database stops counting as in use. The DB branch itself stays until `sidetrack delete` or `gc` (once the git branch is deleted or merged).
- **Protection:** sidetrack won't delete or rename a database another worktree is using, and `gc` keeps it.

Detected migration tools: Alembic, Django, Prisma, Rails, Flyway and Knex. Anything else works too: point `dirs` at the migration folder(s) and set `command`.

## Desktop app (preview)

`crates/sidetrack-app` is a Tauri 2 app over the same core. It opens on **All projects**: every project registered on this machine, with the branch, the database it uses, and anything that needs attention. When a project's database doesn't match its branch, the app shows the switch plan as a proof, step by step. **Apply** runs it, with the same rules as the CLI:
- **Same plan:** before it runs, the plan is built again, and nothing runs if it no longer matches the one on screen.
- **Destructive plans:** these need an explicit confirmation.

**Add project** (in the column, or on the empty screen) adds a repository from the app: pick its folder and the app shows what `sidetrack init` would set up. That's the database, the variable it rewrites, the migration tool and the `sidetrack.toml` it writes. It can start the database if it isn't running, and can install the git hook.

Add project also offers **Choose Compose file** and **Use proxy routing**, including when the default setup can't be read. Proxy setup shows the host, port and database to configure in your app; preserve any JDBC prefix, credentials and URL options. Add the project to save its setup and start the proxy, then update the app connection and recreate its containers. The database must publish a port for the proxy, or have `routing.upstream` configured. Existing `sidetrack.toml` settings take precedence.

The column on the left folds away with the button before the wordmark, or Ctrl+B (⌘B on a Mac), to give the plan the whole window.

**Open project** shows one project as two linked panes: its git branches on the left (with their last few commits and the worktree each is checked out in), and its database branches on the right as a parent → child tree, with snapshots and which worktree uses each database. Selecting a branch on one side selects its partner on the other. Git branches with no database yet, and databases whose git branch is gone, are marked as such. The foot of the column shows whether the database and app containers are running.

The other operations live in that view, where they apply: select a database to snapshot it, reset it to its parent or to one of its snapshots, rename it or delete it; select a git branch with no database to create one (from the closest match, a database you pick, or empty); **Clean up** runs `gc`. Each shows its plan as a proof first, the same plan the CLI would show. Plans that drop a database mark the drops in red and keep **Apply** disabled until you tick "This can't be undone".

```bash
cd crates/sidetrack-app
npm install
npm run tauri dev      # the app, against your real registry
npm run dev            # the UI alone in a browser, with demo data
```

It needs the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (WebKitGTK on Linux, WebView2 on Windows).

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
crates/sidetrack-app    the desktop app (Tauri 2; React + Vite frontend in ui/)
tests/e2e               a compose fixture and the end-to-end script
```
## Ackowledgement

A big thanks to [@shahadulhaider vai](https://github.com/shahadulhaider) for the idea of this project
