# Handoff: where things stand and what's next

Last updated 2026-11-03. Read `CLAUDE.md` first, then this.

## State

| What | Where |
|---|---|
| v1 CLI (Postgres, `.env` routing, hook, snapshots, gc, …) | merged (PR #2) |
| v1.1: bench, disk guard, schema summary, rename, snapshot retention | merged (PR #3) |
| Version-table readers (`readers.rs`, `tests/e2e/alembic.sh`) | merged (PR #4) |
| **v2 local proxy** (`proxy.rs`, `sidetrack proxy`, `routing.mode = "proxy"`) | **PR #5, branch `feat/proxy`.** Lint, 63 unit tests, `run.sh` and `proxy.sh` all passed. Merge it before starting new work. |

`main` is protected: every change goes through a PR (admins included). No approvals are required, and force-pushes and deletion are blocked.

Stale remote branches that can be deleted: `docs/v1-spec`, `feat/v1-cli`, `docs/claude-md-branch-rules`, `feat/v1.1`, `feat/migration-readers`, `claude/serene-volta-pj0vti`.

## Getting going on a new machine

```bash
git clone git@github.com:Abrar118/migration-helper-cli.git && cd migration-helper-cli
cargo test                               # Postgres tests skip without SIDETRACK_TEST_PG
docker run -d --name st-pg -e POSTGRES_PASSWORD=x postgres:16-alpine
SIDETRACK_TEST_PG="docker exec -i st-pg psql -U postgres" cargo test
tests/e2e/run.sh
tests/e2e/proxy.sh
python3 -m venv .venv && .venv/bin/pip install alembic "psycopg[binary]"
PATH="$PWD/.venv/bin:$PATH" tests/e2e/alembic.sh
```

Notes for macOS:
- **`tests/e2e/run.sh`** uses `script` for its terminal-prompt step, with util-linux syntax (`script -qec`). BSD `script` takes different flags, so that one step may need a tweak.
- **`tests/e2e/proxy.sh`** relies on `host.docker.internal`, which Docker Desktop provides. It hasn't been run on a Mac yet.

## How the proxy works (v2, done)

See plan-v1.md, "Local proxy (v2)", decisions 17–20.
- **Target:** the active DB branch is one line in `.git/sidetrack-proxy-target`. `switch` writes it with `Step::SetProxyTarget`.
- **Rerouting:** the proxy swaps `database = routing.database` in the startup message, then relays bytes untouched.
- **Moving connections:** a watcher closes the app's connections to the old database when the target changes.
- **Restarts:** a plain switch restarts nothing. Copying the active database still stops the app services and starts them again.

### Proxy follow-ups (not done)

- **Running it in the background:** `sidetrack proxy` only runs in the foreground. Options:
  - `--daemon` with a pidfile;
  - a launchd / systemd unit;
  - running it as a compose service (the app would then reach it by service name, and `upstream` would be `db:5432`).
- **`init --proxy`:** `init` still detects and writes env mode. Switching to proxy mode means editing `sidetrack.toml` by hand (see the README).
- **Copying without stopping the app:** the proxy could hold new connections to the source while the engine terminates the open ones. That would avoid the stop / start around copies.
- **Git worktrees** (spec P2): several DB branches active at once. Needs a target per worktree, plus a way for each app to tell which one it is (e.g. one listen port per worktree). `git_path` is already per worktree.
- **TLS:** not supported. The proxy answers "no" to SSL requests, so `sslmode=require` clients fail.

## Open question for the owner

**Copy speed on the Mac (Docker Desktop).** Run `sidetrack bench` against a real ~2 GB project database and note the time. Linux numbers are in `plan-v1.md`: FILE_COPY took 3.9 s (13 s right after loading), WAL_LOG 11.9 s.

## Later, per the spec

- Tauri desktop app (v1.5), which reuses `sidetrack-core` and the stable `--json` plan format.
- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
- Approving plans from the desktop app.
