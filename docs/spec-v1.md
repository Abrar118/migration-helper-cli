# Spec: sidetrack — database branches for local Docker development (v1)

Status: Draft · Owner: @abrar118 · Last updated: 2026-11-03

Tool name: **`sidetrack`**.

---

## 1. Problem statement

Developers who switch git branches often leave their local database behind. The schema in a local Docker database reflects whichever branch last ran migrations, not the branch that is checked out.

Example: on branch A you apply 2 migrations. You switch to branch B, which applies 4 more. You switch back to A. Now the migration tool (Alembic, Flyway, …) fails, because the database contains revisions that A doesn't know about. Today the fix is manual: check out B again, downgrade one revision at a time, then switch back. It takes minutes to hours each time. It also only works when every migration has a working down step, and many tools have no down migrations at all (Prisma, Flyway Community).

The cost is lost focus time on every context switch, and a reason developers avoid reviewing other branches locally.

## 2. Prior art (why build this)

| Tool | What it does | Why it isn't enough |
|---|---|---|
| pgbranch | Postgres snapshot per branch using template databases, optional git hook | Postgres only, no handling of never-seen branches, no detection of tools inside Docker |
| Stellar | Manual snapshot/restore for Postgres (partial MySQL) | Python, not git-aware, maintenance unclear |
| actual_db_schema | Rolls back "phantom" migrations on branch switch | Rails only, depends on down migrations |
| Neon Local | Real DB branch per git branch | Requires Neon cloud, not your own Docker DB |

**Gap:** nothing gives a database branch per git branch for any migration tool, with a database running in local Docker. Nothing handles a git branch the tool has never seen correctly. And nothing shows what will happen before it acts.

## 3. Core idea

**Database branches, not migration rollbacks.** Every git branch gets its own real database, named after the branch (`feature/auth` becomes `feature_auth_db`). Switching git branches switches which database the app uses. Nothing is undone, so down migrations are never needed and no data is destroyed.

**Branches and snapshots use the same operation.** A *branch* is a live, writable copy of a database. A *snapshot* is a frozen copy kept only to restore from. Both are "copy a database" underneath.

**Plan, then apply.** Every operation first builds a `Plan`: a data object listing steps, costs and warnings. The CLI prints it, the git hook asks about it, and later the Tauri UI renders it. Only the apply step changes anything.

## 4. Goals

1. **Remove manual migration repair on branch switch.** Target: zero manual `alembic downgrade` (or equivalent) during 2 weeks of daily use.
2. **Fast round trip.** Switching to a branch that already has a DB branch takes ≤ 10 s, not counting the app's own restart. Creating a new DB branch from a 2 GB Postgres database takes ≤ 60 s on ext4 and ≤ 5 s on a copy-on-write filesystem with Postgres 18 clone.
3. **Zero-config start.** From install to first successful switch in ≤ 5 minutes on a standard docker-compose project, with only confirmation prompts and no manual config editing.
4. **Never surprise the user.** 100% of changes are shown as a plan and confirmed (or explicitly pre-approved) before they run.
5. **Never break git.** The git hook can never fail, block or corrupt a checkout.

## 5. Non-goals (v1)

| Non-goal | Why |
|---|---|
| Databases other than Postgres | Prove the model on one engine. MySQL comes next. The engine adapter interface gets extracted once there are 2 real implementations. |
| Local proxy / transparent routing | v2. `.env` rewrite plus an app restart is enough to validate the workflow. |
| Tauri desktop UI | v1.5. It needs a stable core and plan format first. |
| Merging DB branches | Two databases can't be merged meaningfully. After a git merge, migrate the target branch forward. |
| Running without Docker | Every target project runs its DB in Docker. Supporting host-installed databases doubles the detection and execution paths. |
| Production or shared/remote databases | This tool copies and drops databases. It must only ever touch local development containers. |
| MongoDB / Cassandra | Little or no schema-migration drift problem. Revisit only on real demand. |

## 6. Personas & user stories

**Primary: a backend developer** who juggles several feature branches, runs Postgres and the app in docker-compose, and runs migrations inside the app container (e.g. Alembic not installed on the host).

**Secondary: a teammate who avoids the CLI.** Served by the ask-first prompts in v1 and the Tauri app in v1.5.

Stories, in priority order:

1. As a backend dev, when I `git switch` to a branch I've worked on before, I want my DB switched to that branch's database, so my migration tool sees exactly the revisions it knows.
2. As a backend dev, when I switch to a branch I've never used, I want a DB branch created from the nearest compatible ancestor and migrated forward, so I can start working without rebuilding from scratch.
3. As a backend dev, I want to see what will happen (DB to create, migrations to run, containers to restart, disk cost) before anything runs, so I stay in control.
4. As a backend dev, I want `sidetrack init` to find my DB container, credentials, migration tool and the container that runs it, so I don't write config by hand.
5. As a backend dev, I want to take a snapshot before a risky migration and reset to it, so I can iterate on migrations cheaply.
6. As a backend dev, I want DB branches for deleted or merged git branches to be offered for cleanup, so Docker doesn't run out of disk.
7. As a backend dev using a git GUI (VS Code, GitKraken), I want branch switches to keep working and to be told when my DB is out of sync, instead of a hung or failed checkout.
8. **Edge:** as a backend dev, if the tool's records disagree with reality (e.g. I ran `docker compose down -v`), I want that shown and explained, not silently "fixed".
9. **Edge:** as a backend dev, if a switch fails halfway, I want things returned to the state before the switch.

## 7. Requirements

### P0 — must have for v1

#### P0.1 DB branch naming

The git branch name is mapped to a DB name deterministically:

1. Lowercase.
2. Replace every character outside `[a-z0-9_]` with `_`, collapse repeated `_` and trim `_` from both ends.
3. If the result starts with a digit, prefix `b_`.
4. Append `_db`.
5. If longer than 63 bytes (the Postgres limit), truncate and insert `_` + the first 6 hex characters of the SHA-256 of the original branch name before `_db`.
6. If the name collides in the registry with a *different* git branch (e.g. `feature/Foo` and `feature-foo`), use the hashed form for the newer branch.

The existing database at `sidetrack init` is **adopted** as the DB branch for the current git branch under its existing name (e.g. `app_dev` ↔ `dev`). It is not renamed.

Acceptance:
- [ ] `feature/auth-v2` → `feature_auth_v2_db`; `main` → `main_db`; `123-fix` → `b_123_fix_db`
- [ ] A 100-character branch name produces a valid name of ≤ 63 bytes, and running it again gives the same name
- [ ] `feature/Foo` and `feature-foo` never map to the same database

#### P0.2 Registry (local SQLite)

- **Location:** platform data directory (`~/.local/share/sidetrack/registry.sqlite` on Linux, `~/Library/Application Support/sidetrack/registry.sqlite` on macOS). One file for all projects, so the future UI can show them all.
- **Tables (minimum):**
  - `projects` (repo root, compose project, engine, DB service, adopted base DB)
  - `db_branches` (project, git branch, DB name, parent DB branch, base commit, migration-set hash, created/last-used, status)
  - `snapshots` (DB branch, DB name, commit, migration file set, size, created)
  - `operations` (the plan JSON, result, timestamps), used for auditing and recovery
- **The DB server is the source of truth.** Every command starts with a reconcile step: list the databases that actually exist on the server and compare them with the registry. Any mismatch (missing DB, unknown `*_db` database) appears as a warning in the plan and is never auto-fixed.

Acceptance:
- [ ] After `docker compose down -v`, `sidetrack status` reports the DB branches as missing and offers to clear them from the registry or recreate them
- [ ] Deleting the registry file loses no databases. `sidetrack init` can re-adopt existing `*_db` databases.

#### P0.3 Detection (`sidetrack init`)

Detection runs once, shows its findings as a plan, and writes the result to `sidetrack.toml` in the repo (committable, no secrets).

1. **Compose:** run `docker compose config --format json`, which resolves overrides, profiles, `include`, `env_file` and `${VAR}` interpolation. Never parse the YAML ourselves. Use `docker compose ps --format json` for what's running.
2. **DB service:** a service whose image matches `postgres`, `postgis/postgis`, `timescale/timescaledb*`, `bitnami/postgresql` or `supabase/postgres`. Read the user and DB from `POSTGRES_USER` / `POSTGRES_DB` (and their `_FILE` variants, read inside the container).
3. **App services:** services that `depends_on` the DB service, or whose environment refers to the DB service's hostname.
4. **Connection variable:** in the app service's environment and its `env_file` entries, find the variable that holds the DB name. Either a URL whose path is the current DB name (`DATABASE_URL`, `SQLALCHEMY_DATABASE_URI`, …) or a variable whose value equals it (`DB_NAME`, `POSTGRES_DB`, …). Record which **file** defines it.
5. **Migration tool:** from repo files.
   - P0: Alembic (`alembic.ini`) and a generic fallback.
   - Detected for messaging only (Django, Flyway, Prisma, Rails, Knex, golang-migrate) but treated as generic in v1.
6. **Where migrations run:** the app service whose build context or bind mount contains the migration config. If that service is running, verify with `docker compose exec -T <svc> alembic --version` (only with the user's consent). Result, e.g.: `docker compose exec -T api alembic upgrade head`.
7. **Migrations directory:** recorded so the tool can read migration file sets from git (see P0.6).

Acceptance:
- [ ] For a compose project with `db` (postgres), `api` (Alembic installed only in the container) and `DATABASE_URL` in `.env`, `sidetrack init` writes a correct `sidetrack.toml` with no manual edits
- [ ] If any value can't be determined, `init` asks for it instead of guessing

`sidetrack.toml` sketch:
```toml
[database]
engine  = "postgres"
service = "db"
user    = "postgres"

[routing]
mode     = "env"            # v2: "proxy"
file     = ".env"
variable = "DATABASE_URL"   # db name is the URL path
restart  = ["api", "worker"]

[migrations]
tool    = "alembic"
dir     = "alembic/versions"
command = "docker compose exec -T api alembic upgrade head"
# seed  = "docker compose exec -T api python -m app.seed"   # used when building from empty
```

#### P0.4 Running SQL

v1 runs all SQL via `docker compose exec -T <db> psql -v ON_ERROR_STOP=1 -U <user>`. That avoids published-port, host-credential and TLS problems, and the host needs no Postgres client. A native Rust driver can come later.

#### P0.5 Postgres engine operations

| Operation | Implementation |
|---|---|
| list | `SELECT datname FROM pg_database` |
| clone (branch or snapshot) | `CREATE DATABASE <new> TEMPLATE <src> STRATEGY FILE_COPY` (PG ≥ 15). On PG 18, use `file_copy_method = clone` when the plan probe finds support. Fall back to plain `TEMPLATE` on older versions. |
| terminate connections | `pg_terminate_backend` for the source DB (required: Postgres refuses to copy a database that has connections) |
| drop | `DROP DATABASE … WITH (FORCE)` (PG ≥ 13) |
| freeze snapshot | `ALTER DATABASE <snap> ALLOW_CONNECTIONS false` so nothing writes to it |

Snapshot DB naming: `<branch_db>__s<id>` (same length and hash rules).

#### P0.6 Choosing the base for a new DB branch (gap 3)

When the target git branch has no DB branch:

1. Find each candidate DB branch's or snapshot's **migration file set**: the file names in `migrations.dir` at its recorded commit, read through `git ls-tree`.
2. Compute the target branch's migration file set at `HEAD`.
3. A candidate is **compatible** if its set is a subset of the target's set. It holds no migrations the target doesn't know.
4. Prefer, in order:
   - candidates at the target's `git merge-base` with each known branch,
   - then the newest compatible snapshot,
   - then the compatible live DB branch with the largest set.
5. Clone it, then run `migrations.command` to migrate forward.
6. If nothing is compatible, offer **build from empty**: create an empty DB, run migrations, run `seed` if configured.

This uses only git and the file system, so it works for any migration tool that stores one file or folder per migration.

Acceptance:
- [ ] Scenario from §1: returning to A selects `a_db` (exists) and runs no migrations
- [ ] A new branch C forked from `dev` *before* `dev` gained migration X never uses a base containing X
- [ ] The plan names the chosen base and why, and lists the migrations that will run forward

#### P0.7 Routing via `.env` (v1)

- Rewrite only the single matching line in the detected file. Keep comments, order and quoting. Replace only the DB-name part of a URL.
- **Refuse** if the file is tracked by git (`git ls-files --error-unmatch`). Suggest moving the variable into an untracked file.
- Store the previous value in the `operations` log so it can be undone.
- Apply with `docker compose up -d --no-deps <restart services>`. **Not** `docker compose restart`, which doesn't reload `env_file`.
- Order matters: point the app at the new DB **first**, then run migrations, so containerised migration tools connect to the right database.

#### P0.8 Plan / apply engine

- A `Plan` is a list of typed steps, e.g.:
  - `ReconcileWarning`, `TerminateConnections`, `CloneDatabase`, `RewriteEnv`, `RestartServices`, `RunMigrations`, `CreateSnapshot`, `DropDatabase`, `UpdateRegistry`
- Each step carries an estimate (time, bytes) and whether it can be undone.
- A plan also shows:
  - the migration files to run forward,
  - services to restart and connections to close,
  - extra disk use (from `pg_database_size`).
- **Apply** runs steps in order. On failure, it undoes completed steps that can be undone (drop half-created DBs, restore the `.env` value) in reverse order, then reports which step failed and the exact error.
- Every command supports `--dry-run` (print the plan and exit) and `--json` (machine-readable plan and result, for scripts and the future UI).

#### P0.9 Ask first

- Prompt: `[Y]es / [n]o / [a]lways for this repo`. "Always" is stored in local, non-committed config.
- Destructive steps (`DropDatabase`, terminating connections from a non-app client) always prompt, even with "always". Use `--yes` to skip.

Example:
```text
Switched to feature/auth. DB is on dev_db.
Plan:
  1. Close 3 connections to dev_db (api)
  2. Create feature_auth_db from snapshot dev_db__s4 (dev @ a1b2c3)   ~15s, +2.1 GB
  3. Set DATABASE_URL → …/feature_auth_db in .env
  4. Recreate api
  5. Run 2 migrations: 3f9a_add_users_table, 7c21_add_sessions
Apply? [Y]es / [n]o / [a]lways for this repo
```

#### P0.10 Git hook

- `sidetrack hook install` adds a `post-checkout` hook. It chains with any existing hook and doesn't replace it.
- The hook does nothing when:
  - it's a file checkout (`$3 == 0`), or the old and new HEAD are the same,
  - a rebase, merge, cherry-pick or bisect is in progress,
  - HEAD is detached (prints a one-line note),
  - `SIDETRACK_SKIP=1` is set.
- **No TTY** (e.g. a GUI client): print one line, `sidetrack: DB still on dev_db — run 'sidetrack switch'`, and exit.
- **Always exits 0.** Errors are printed, never raised.
- Must finish in under 1 s when nothing applies. Must not block when Docker is down (print a note and exit).

#### P0.11 Commands

| Command | Purpose |
|---|---|
| `sidetrack init` | Detect, confirm, write `sidetrack.toml`, adopt the current DB, optionally install the hook |
| `sidetrack status` | Current git branch ↔ active DB branch, drift warnings, reconcile results |
| `sidetrack list` | DB branches and snapshots with git branch, size, last used, migration count |
| `sidetrack switch [branch]` | Plan and apply a switch (defaults to the current git branch) |
| `sidetrack create <branch> [--from <db-branch\|snapshot>\|--empty]` | Explicit creation |
| `sidetrack snapshot [name]` / `sidetrack snapshot list` / `sidetrack snapshot rm` | Manage snapshots of the active DB branch |
| `sidetrack reset [--to <snapshot>\|--parent]` | Replace the active DB branch with a fresh copy |
| `sidetrack delete <branch>` | Drop a DB branch and its snapshots |
| `sidetrack gc` | Offer to delete DB branches whose git branch is deleted or merged, and old snapshots |
| `sidetrack hook install\|uninstall` | Manage the git hook |
| `sidetrack doctor` | Check Docker, compose, Postgres version, clone support, disk space, `.env` tracking |

Global flags: `--dry-run`, `--json`, `--yes`.

### P1 — fast follow

- ✅ **Schema diff in plans** (v1.1): table and column summary of the current vs. target DB, from `information_schema` rather than `pg_dump`. Column detail with `-v`.
- ✅ **Disk guard** (v1.1): plans that won't fit are refused; plans that leave < 1 GB or 10% free warn.
- ✅ `sidetrack rename` that mirrors `git branch -m` (v1.1).
- ✅ Snapshot retention policy (v1.1): `[snapshots] keep = N`, applied by `gc`; labelled snapshots are kept.
- ✅ `sidetrack bench` (v1.1, added): times a real copy on this machine for the §10 spike.
- ✅ Version-table readers (added after v1.1): Alembic (heads + `down_revision` chain), Django, Flyway, Rails, Prisma, Knex, or any table via `version_table` / `version_column`. A database's real applied set replaces sidetrack's records for base selection, pending migrations, warnings and `status`.
- Clone-capability probe that tells the user how to get instant clones (PG 18 on btrfs/XFS/APFS).

### P2 — design for, don't build

These are reasons for the architecture decisions below:

- **Local proxy routing (v2):** the app always connects to its normal DB name, and the proxy rewrites the database name in the Postgres startup message. Routing must therefore be behind a `Router` interface (`env` now, `proxy` later).
- **Tauri desktop app (v1.5):**
  - Read-only maps of git branches, DB branches, containers and commits; plan previews and schema diffs.
  - Later, approvals: the hook sends the plan to the running app over a local socket when there is no TTY.
  - This is why the core is a library, plans are serialisable, and the registry is global.
- **More engines, in this order:** MySQL/MariaDB (InnoDB transportable tablespaces, needs a spike) → SQL Server (backup/restore for branches, native database snapshots for resets) → Oracle (pluggable database clone). The engine interface is extracted when MySQL lands, not before.
- **Git worktrees:** several DB branches active at once. Needs proxy routing.

## 8. Architecture

```text
crates/core     # library: git, docker/compose, detection, registry, naming,
                #          planner, applier, engine::postgres, router::env
crates/cli      # thin: clap args → core plan → render/prompt → core apply
apps/desktop    # (v1.5) Tauri, depends on core
```

Key decisions:
- Core logic lives in a library, not the CLI binary.
- Plans are data (serde) and the applier is the only code that touches anything.
- SQL runs via `docker compose exec`.
- The registry is a SQLite file (`rusqlite`).
- Git operations use the `git` CLI (it's always present and matches the user's git config). `gix`/`git2` is optional later.

## 9. Success metrics

Measured by dogfooding first: the author's daily work, then 2–3 teammates.

**Leading (first 2 weeks):**
- Manual migration repairs after a branch switch: **0** (baseline: several per week)
- Median time for `sidetrack switch` to an existing DB branch: **≤ 10 s**
- New DB branch from a 2 GB Postgres base: **≤ 60 s** (ext4) / **≤ 5 s** (clone-capable)
- Checkouts broken or blocked by the hook: **0**
- Install to first successful switch: **≤ 5 min**

**Lagging (1–3 months):**
- Teammates still using it after a month: ≥ 2 of 3
- Wrong-base incidents (new branch got a DB containing unknown migrations): 0
- Disk-full incidents caused by the tool after `gc` shipped: 0

## 10. Risks

| Risk | Mitigation |
|---|---|
| Copying is slow on Docker Desktop for Mac (VM disk, no reflink) | **Spike first:** time a 2 GB `CREATE DATABASE … TEMPLATE` on Docker Desktop (Mac) and on Linux ext4 before building further |
| Can't copy while the app is connected | Plan step that closes connections, explicitly shown and confirmed |
| Detection guesses wrong (wrong `.env` or variable) | Detection results are always confirmed and written to an editable `sidetrack.toml`. Never guess silently at runtime. |
| Registry drifts from reality | Reconcile on every command, with the DB server as source of truth |
| Disk fills with 2 GB copies | `gc`, plan cost estimates, disk guard (P1) |
| Pointing at a non-local DB | Only act on containers from the current compose project. Refuse if the DB service has no local container. |

## 11. Open questions

| # | Question | Owner | Blocking? |
|---|---|---|---|
| 1 | ~~Final product / binary name~~ Decided: `sidetrack` | Stakeholder | Done |
| 2 | Minimum supported Postgres version: 13 (`DROP … FORCE`) or 15 (`STRATEGY FILE_COPY`)? | Engineering | Yes, before P0.5 |
| 3 | How fast is a 2 GB template copy on Docker Desktop for Mac in practice? (spike) | Engineering | Yes, sets expectations for goal 2 |
| 4 | Should `sidetrack.toml` be committed and shared with the team, with only personal settings local? | Stakeholder | No |
| 5 | Monorepos with several DB services: one project per DB service, or several engines per project? | Engineering | No (v1: one DB service) |
| 6 | Windows support: WSL only, or native Docker Desktop on Windows too? | Stakeholder | No |
| 7 | Default snapshot policy: automatic snapshot before each forward migration run, or only on demand? | Engineering | No |

## 12. Phasing

| Phase | Scope |
|---|---|
| **v0 — spike (≤ 2 days)** | Template-copy timing on Mac and Linux; naming function with tests; prototype of detection via `docker compose config` against 2 real projects |
| **v1 — CLI, Postgres** | All P0 items. Alembic plus the generic migration file-set reader. |
| **v1.1** | P1 items: schema diff, disk guard, rename, retention, more migration-tool readers |
| **v1.5 — Tauri (read-only)** | Branch / DB / container / commit maps, plan previews, schema diffs |
| **v2** | Proxy routing, Tauri approvals (hook → app over a socket), worktrees, MySQL/MariaDB |
| **Later** | SQL Server, Oracle; MongoDB / Cassandra only on demand |
