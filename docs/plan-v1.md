# Implementation plan: sidetrack v1 (CLI, Postgres)

Spec: [`spec-v1.md`](./spec-v1.md). This doc covers **how** v1 gets built, in order.

## Workspace layout

```text
Cargo.toml                    # workspace
crates/sidetrack-core/        # library: all logic, no terminal I/O
  src/naming.rs               # git branch → DB name (P0.1)
  src/config.rs               # sidetrack.toml model + load/save
  src/registry.rs             # SQLite registry (P0.2)
  src/git.rs                  # wrapper over the git CLI
  src/compose.rs              # wrapper over `docker compose` + config JSON model
  src/detect.rs               # `init` detection (P0.3)
  src/envfile.rs              # read/rewrite one variable in a .env file (P0.7)
  src/migrations.rs           # migration file sets from git + base selection (P0.6)
  src/engine/postgres.rs      # Postgres operations through psql (P0.4, P0.5)
  src/plan.rs                 # Plan + Step types (serde) and rendering helpers
  src/planner.rs              # builds plans for switch/create/snapshot/reset/delete/gc
  src/apply.rs                # runs a plan, undoes completed steps on failure
  src/workspace.rs            # loads config + registry + engine for one repo
crates/sidetrack/             # binary `sidetrack`: clap, prompts, printing
  src/main.rs
  src/hook.rs                 # post-checkout hook install/uninstall/run (P0.10)
  src/ui.rs                   # prompts, approval, output
tests/e2e/                    # docker-compose fixture + script for the §1 scenario
```

## Milestones

| # | Milestone | Done when |
|---|---|---|
| 1 | Workspace, naming, config, envfile | Unit tests for every naming example in the spec, `.env` rewrite keeps comments/quotes |
| 2 | Git + compose wrappers, Postgres engine | Engine test against a real Postgres container: list, clone, freeze, rename, drop |
| 3 | Registry | Schema created with `user_version` migrations, CRUD tests on a temp file |
| 4 | Migration file sets + base selection | Tests on temp git repos covering the "dev moved on after fork" case |
| 5 | Plan, planner, applier | Planner tests (pure inputs) + applier undo test |
| 6 | Detection + `init` | Fixture compose projects (Alembic in container, Django-style, env var in compose file) |
| 7 | CLI commands + git hook | `--dry-run`, `--json`, `--yes`, prompt `[Y/n/a]`, hook always exits 0 |
| 8 | End-to-end | Script reproduces the spec §1 scenario in Docker and passes |

## Decisions made while planning

These refine the spec; each is noted in code where it applies.

1. **Stop the app before copying the active DB.** Postgres refuses `CREATE DATABASE … TEMPLATE x` while anything is connected to `x`. Killing connections alone races with app reconnects, so when the source is the active DB, the plan stops the app services first (`docker compose stop`) and starts them again with the new `.env` (`up -d --no-deps --force-recreate`). The clone still terminates leftover connections and retries up to 3 times.
2. **The active DB is read from the `.env` file**, not from the registry. The file is what the app actually uses, so it's the truth. The registry just maps names to branches.
3. **Base selection ranks by migration file set.** A candidate (live DB branch or snapshot) is compatible when its file set is a subset of the target's. Among compatible candidates, the one with the most files wins (fewest migrations to run forward). Ties go to the most recently used. For live DB branches, the set is `recorded ∪ files at the tip of its git branch`. Over-counting is safe because it only makes a candidate less likely to be picked.
4. **A failed migration is not rolled back.** If the switch itself fails (clone, `.env` rewrite, restart), completed steps are undone. If the user's migration fails afterwards, the new DB branch stays selected so it can be debugged, and the error says so. That's a failure in the user's code, not in the switch.
5. **Reset never drops before the replacement exists.** Order: clone source → `<db>__tmp`, then drop the old DB, then rename tmp → `<db>`. A failure before the drop leaves everything intact.
6. **Migration directories are a list** (`dirs = [...]`) so Django-style projects with one `migrations/` folder per app work in v1.
7. **SQL runs via psql with a configurable command prefix.** By default it's `docker compose exec -T <service> psql -U <user>`. Tests swap in a direct `psql` against a test container. There is no Postgres driver dependency.
8. **"Always approve" is stored in the registry** (per project, on this machine), never in the committed `sidetrack.toml`.

### Found during implementation

9. **Two migration sets per candidate, not one.** "May contain" (recorded ∪ files at its branch tip) decides compatibility, where over-counting is safe. "Known applied" (recorded only) decides what to run, where under-counting is safe because the tool skips applied migrations. Using one set for both skipped needed migrations when a branch's local DB was behind its git branch (caught by the e2e test).
10. **The hook does not skip when old HEAD == new HEAD.** The spec said to skip it, but `git checkout -b feature` keeps the same commit and still needs its own database. The hook stays quiet anyway when the plan has nothing to do.
11. **`gc` only treats a branch as merged if it moved since its DB branch was created.** Git reports a brand-new branch with no commits as "merged", which would have dropped fresh databases.
12. **Detection reads three compose views:** resolved (final values), `--no-env-resolution` (which `env_file` a value comes from), and `--no-interpolate` (`${VAR}` references, to find the variable in the project `.env`).
13. **Postgres 18 "clone" isn't always copy-on-write.** `file_copy_method = clone` falls back to a normal copy on filesystems without reflinks (seen on overlayfs), so it's reported as "Clone", not as instant.

## v1.1 (fast follows)

| Feature | Where |
|---|---|
| `sidetrack bench [--from X]`: copy, time, drop the copy; estimates for 1–10 GB | `Planner::bench`, `bench_summary` in the CLI |
| Disk guard: refuse plans that won't fit, warn under 1 GB / 10% free | `Planner::guard_disk`, `Workspace::disk_space` (`df` in the DB container) |
| Schema summary on switch plans | `schema.rs`, `Workspace::annotate_schema` |
| `sidetrack rename <old> <new>` | `Planner::rename`, `Step::RelinkBranch` |
| `[snapshots] keep = N` retention in `gc` | `Planner::push_retention` |

Measurements so far (Linux sandbox, overlayfs, Postgres 16, 1.9 GB pgbench database):
- `STRATEGY FILE_COPY`: 13 s right after loading the data (the checkpoint dominates), then 3.9 s.
- `WAL_LOG`: 11.9 s.

Copies of databases under 50 MB are dominated by fixed costs, so `bench` gives no estimates for them. The Mac / Docker Desktop number is still open; it's what `sidetrack bench` is for.

Decisions:

14. **Bench drops its copy with `DropTemporaryDatabase`**, a non-destructive step, so it doesn't trigger the "this deletes data" prompt.
15. **The schema summary is best effort.** It's attached after planning (the planner stays pure), only for switches to an existing DB branch, and silently skipped if a query fails.
16. **Renaming an adopted database keeps its name**; only the git link moves. Other DB branches get the new branch's name.

## Version-table readers

sidetrack used to infer a database's migrations from its own records plus git. That was cautious enough to be safe, but it went wrong in one common case. When `main` gained a migration in git that the local `app_dev` never ran, branches from an older `main` couldn't reuse `app_dev` and were built from empty.

Readers ask the database instead (`readers.rs`, `Workspace::read_migrations`):
- **One psql session reads every connectable DB branch.** It's a script on stdin: `\connect`, then `\gset` / `\if` so the table is only queried where it exists.
- **Rows are mapped to migration file keys** using a catalog of files from the working tree and every local branch.
- **Alembic stores only head revisions**, so the catalog also holds each file's `revision` / `down_revision`, read in one `git cat-file --batch` call. Applied = the heads and all their ancestors.
- **Rows that match no known file are "unknown".** They make that database incompatible with every branch, and show in warnings and `status`.
- **Django rows for apps without a migrations folder in the repo** (`auth`, `admin`, …) are ignored.
- **A reader failure never blocks planning.** sidetrack falls back to its records and `status` says why.

`tests/e2e/alembic.sh` checks this with real `alembic revision` files.

## Local proxy (v2)

`routing.mode = "proxy"` routes the app through `sidetrack proxy` instead of rewriting an env file (`proxy.rs`).
- **The target** (the active DB branch) is one line in `.git/sidetrack-proxy-target`. `switch` writes it with the `SetProxyTarget` step. If the file is missing, the target is `routing.database` itself. In proxy mode, `Workspace::active_db` reads this file, not the env file.
- **The proxy** uses std threads and blocking sockets, with no async runtime.
  - It answers SSL/GSS encryption requests with "no" and reads the startup message.
  - If `database` equals `routing.database`, it swaps in the target.
  - It then relays bytes both ways. Authentication passes through untouched.
  - Cancel requests are forwarded as they are.
- **Moving open connections:** a watcher polls the target file every 250 ms. When the target changes, it closes the app's connections to the old database, so connection pools reconnect to the new one. Connections to any other database name are never touched.
- **Upstream** is `routing.upstream`, or else the port compose publishes for the database service (`docker compose port db 5432`).

Decisions:

17. **The target lives in a file under `.git`, not in the registry.** The proxy re-reads it for each connection with no SQLite locking, it's per checkout, and it is never committed.
18. **Copying the active database still stops the app.** `CREATE DATABASE … TEMPLATE` needs no connections to the source, and an app's connection pool would reconnect through the proxy straight away. Only services stopped for a copy are started again. A plain switch needs no restart.
19. **No TLS between app and proxy.** It's local traffic. Clients with `sslmode=require` fail, and the docs say to use `prefer` or `disable`.
20. **Plans warn when the proxy isn't running** (`World.proxy_running`). They don't refuse, because the target can be set before the proxy starts.

21. **`proxy --daemon` re-runs the binary in the background.** It's a child process in its own process group, so Ctrl-C in the terminal doesn't stop it, with stdout/stderr going to `.git/sidetrack-proxy.log`. The pid goes in `.git/sidetrack-proxy.pid` once the port accepts connections. `--stop` sends `kill` to that pid. There's no fork or libc, which keeps it simple, at the cost of being Unix-only for now.
22. **`init --proxy` never edits the app's env file.** It writes proxy-mode config, with `routing.database` read from the detected env variable, and prints the one-time URL change for the user to make.

23. **A running proxy holds connections instead of the app being stopped** (`HoldConnections` / `ReleaseConnections`; supersedes the restart part of decision 18).
    - The hold is a file next to the target (`.git/sidetrack-proxy-hold`: db name and expiry). A new connection that would reach that database waits until the hold is removed, then re-reads the target, so it lands on the new branch.
    - Existing sessions are still terminated before the copy.
    - The hold expires after 10 minutes, in case sidetrack dies before releasing it.
    - Undo removes the hold.
    - Without a running proxy, plans stop the app as before.
24. **Proxy autostart:** before planning, the CLI and the hook start the proxy in the background if it isn't running (`routing.autostart`, default true). It never does this on `--dry-run`. It happens before `world()`, so the plan can already use holds.
25. **Detection ranks Postgres services.** A service with `POSTGRES_*` settings scores 2, and one that other services depend on scores 1. Before, the first one by name won, which picked an app container that only used the postgres image for psql.

`tests/e2e/proxy.sh` covers it all against real Docker. The app container reaches the host proxy via `host.docker.internal` (`extra_hosts: host-gateway` on Linux).

## Git worktrees

Each git worktree is a checkout with its own active DB branch. All worktrees share one database server and one registry project.
- **Shared server:** `Workspace.main_root` is the main worktree's root. The registry key and `db_compose` use it, so the database service, `psql`, `df` and the published port all come from the main worktree's compose project. App services still come from this checkout's compose (`compose`).
- **Per worktree:** the active DB and the proxy's pid, log and target files sit under `git_path`, which is per worktree. Each linked worktree's proxy listens on the next free port after `routing.listen`. The port is chosen once and saved in `sidetrack-proxy-listen` in that worktree's git dir.
- **Shared hold:** the hold file is in the common git dir, so a copy holds every worktree's proxy, not just this one's.
- **`World.elsewhere`** maps databases to the other worktrees using them. It's read from their target files (proxy mode) or env files.
  - `delete` and `rename` refuse those databases, and `gc` keeps them.
  - Copying one holds connections (proxy mode) or notes that its connections will be closed (env mode).
  - Switching to one warns that both apps will share it.

Decisions:

26. **Worktrees share the main worktree's database server.** Otherwise each worktree's compose project, named after its directory, would start its own Postgres, and DB branches couldn't be copied between them. In a linked worktree, start only the app services.
27. **Worktree ports are allocated once and saved**, rather than derived from the worktree's position in `git worktree list`. Positions shift when a worktree is removed, and the app's URL would then point at the wrong proxy.
28. **Env mode works in worktrees too, but proxy mode is the intended setup.** A containerized app in a worktree reaches the database by host (`host.docker.internal:<port>`). With env mode, `db` would resolve to the worktree's own compose network.

`tests/e2e/proxy.sh` adds a worktree: own DB branch, own port (54330), main untouched, delete refused.

## Test strategy

- **Unit tests:** naming, envfile, config round-trip, compose JSON parsing, detection on fixture directories, file-set logic, planner (pure inputs).
- **Git tests:** temp repos created by the tests themselves.
- **Postgres tests:** run when `SIDETRACK_TEST_PG` is set (a psql command prefix), and are skipped otherwise, so `cargo test` works without Docker.
- **End-to-end:** `tests/e2e/run.sh` creates a compose project (Postgres plus a tiny SQL-file migration runner), a git repo with branches A and B, then runs the spec §1 scenario through the real binary and asserts on the tables that exist.
