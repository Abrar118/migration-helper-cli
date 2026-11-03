# Handoff: where things stand and what's next

Last updated 2026-11-03. Read `CLAUDE.md` first, then this.

## State

| What | Where |
|---|---|
| v1 CLI (Postgres, `.env` routing, hook, snapshots, gc, …) | merged (PR #2) |
| v1.1: bench, disk guard, schema summary, rename, snapshot retention | merged (PR #3) |
| Version-table readers (`readers.rs`, `tests/e2e/alembic.sh`) | merged (PR #4) |
| v2 local proxy (`proxy.rs`, `sidetrack proxy`, `routing.mode = "proxy"`) | merged (PR #5) |
| Proxy follow-ups: `proxy --daemon` / `--stop`, `init --proxy` | merged (PR #6) |
| **More proxy follow-ups:** autostart, holding connections during copies, detection with several Postgres images | **PR #7, branch `feat/proxy-followups-2`.** Merge it before starting new work. |

`main` is protected. Every PR needs one approval, including a code-owner review (`@Abrar118`). The owner can bypass the rules. Force-pushes and deletion are blocked.

Stale remote branches that can be deleted: `docs/v1-spec`, `feat/v1-cli`, `docs/claude-md-branch-rules`, `feat/v1.1`, `feat/migration-readers`, `feat/proxy`, `feat/proxy-followups`, `claude/serene-volta-pj0vti`.

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

See plan-v1.md, "Local proxy (v2)", decisions 17–25.
- **Target:** the active DB branch is one line in `.git/sidetrack-proxy-target`. `switch` writes it with `Step::SetProxyTarget`.
- **Rerouting:** the proxy swaps `database = routing.database` in the startup message, then relays bytes untouched.
- **Moving connections:** a watcher closes the app's connections to the old database when the target changes.
- **Restarts:** none. While the proxy runs, a copy of the active database holds the app's new connections in the proxy instead of stopping the app. Commands and the hook start the proxy automatically.

### Proxy follow-ups

- ✅ **`sidetrack proxy --daemon` / `--stop`:** done in PR #6. It's Unix-only, since it uses `process_group` and `kill`.
- ✅ **`init --proxy`:** done in PR #6. It hasn't been tested end to end, because detection picks the wrong service in the e2e fixture, which has two `postgres` images. A test would need a fixture whose app image isn't Postgres.
- ✅ **Starting the proxy automatically:** done in PR #7 (`routing.autostart`). A launchd / systemd unit, or running the proxy as a compose service, is still possible but not needed.
- ✅ **Detection with several Postgres images:** done in PR #7.
- ✅ **Copying without stopping the app:** done in PR #7 (holds). Rename still stops the app. It's rare, and the same `push_pause` helper would cover it.
- **`init --proxy` end to end:** still untested. Detection refuses a DB name written literally in compose `environment:`, because env mode can't rewrite it. In proxy mode that's fine, so `init --proxy` should allow it. Then `tests/e2e/proxy.sh` could run `init --proxy` instead of writing `sidetrack.toml` by hand.
- **Git worktrees** (spec P2, not started): several DB branches active at once. The target, hold, pid and log files already live under `git_path`, which is per worktree. Still missing:
  - one listen port per worktree (e.g. `routing.listen` overridden per worktree, or base port + index);
  - `Workspace::proxy_running` and `--daemon` using that port;
  - each worktree's app pointed at its own port;
  - the registry's project key (repo root) shared across worktrees.
- **TLS** (not started, low priority for local dev): the proxy answers "no" to SSL requests, so `sslmode=require` clients fail. Supporting it means terminating TLS in the proxy with a local certificate (e.g. `rustls` with a self-signed cert) and opening TLS upstream when the server needs it.

## Open question for the owner

**Copy speed on the Mac (Docker Desktop).** Run `sidetrack bench` against a real ~2 GB project database and note the time. Linux numbers are in `plan-v1.md`: FILE_COPY took 3.9 s (13 s right after loading), WAL_LOG 11.9 s.

## Later, per the spec

- **Tauri desktop app (v1.5).** It's the next big item. The plan:
  - a `crates/sidetrack-app` Tauri 2 crate that calls `sidetrack-core` directly (`Workspace::world`, `Planner`, `Applier`), the same way the CLI does;
  - views from the spec: a git branch map, a DB branch tree (parent → child), container status, commit history per branch, and plan / schema diff previews with Apply;
  - prompts from the git hook could go to the app instead of the terminal ("Tauri approvals").
  - Build it on a desktop machine. It needs a GUI to check, and it's frontend-heavy.
- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
- Approving plans from the desktop app.
