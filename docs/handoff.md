# Handoff: where things stand and what's next

Last updated 2026-11-04. Read `CLAUDE.md` first, then this.

## State

| What | Where |
|---|---|
| v1 CLI (Postgres, `.env` routing, hook, snapshots, gc, …) | merged (PR #2) |
| v1.1: bench, disk guard, schema summary, rename, snapshot retention | merged (PR #3) |
| Version-table readers (`readers.rs`, `tests/e2e/alembic.sh`) | merged (PR #4) |
| v2 local proxy (`proxy.rs`, `sidetrack proxy`, `routing.mode = "proxy"`) | merged (PR #5) |
| Proxy follow-ups: `proxy --daemon` / `--stop`, `init --proxy` | merged (PR #6) |
| More proxy follow-ups: autostart, holding connections during copies, detection with several Postgres images | merged (PR #7) |
| Git worktrees (spec P2) | merged (PR #8) |
| Worktree edges: env-file port wiring, env-mode warning, removal checks | merged (PR #9) |
| `init --proxy` accepts a DB name written in compose, plus an e2e check; Windows path fixes | merged (PR #10) |

Nothing is in flight. The next item is the Tauri desktop app (see "Later, per the spec").

`main` is protected. Every PR needs one approval, including a code-owner review (`@Abrar118`). The owner can bypass the rules. Force-pushes and deletion are blocked.

After every PR merge, update this file: the State table, the follow-ups, and the date.

Stale remote branches were deleted on 2026-11-04. Only `main` is on the remote.

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

Notes for native Windows (spec open question 6 is still undecided):
- **Unit tests:** `cargo build` and `cargo test` pass. Detection always writes paths with `/` (`detect::relative`), both repo paths and paths inside containers.
- **Untested:** the e2e scripts, the git hook, and `proxy --daemon` (Unix-only).

## How the proxy works (v2, done)

See plan-v1.md, "Local proxy (v2)", decisions 17–25.
- **Target:** the active DB branch is one line in `.git/sidetrack-proxy-target`. `switch` writes it with `Step::SetProxyTarget`.
- **Rerouting:** the proxy swaps `database = routing.database` in the startup message, then relays bytes untouched.
- **Moving connections:** a watcher closes the app's connections to the old database when the target changes.
- **Restarts:** none. While the proxy runs, a copy of the active database holds the app's new connections in the proxy instead of stopping the app. Commands and the hook start the proxy automatically.

### Proxy follow-ups

- ✅ **`sidetrack proxy --daemon` / `--stop`:** done in PR #6. It's Unix-only, since it uses `process_group` and `kill`.
- ✅ **`init --proxy`:** done in PR #6.
- ✅ **Starting the proxy automatically:** done in PR #7 (`routing.autostart`). A launchd / systemd unit, or running the proxy as a compose service, is still possible but not needed.
- ✅ **Detection with several Postgres images:** done in PR #7.
- ✅ **Copying without stopping the app:** done in PR #7 (holds). Rename still stops the app. It's rare, and the same `push_pause` helper would cover it.
- ✅ **`init --proxy` end to end:** done in PR #10. In proxy mode, detection accepts a DB name written directly in compose `environment:` (`Detection.inline`; plan decision 22). The first step of `tests/e2e/proxy.sh` runs a real `init --proxy` on a small Alembic-shaped project; Alembic itself never runs. The main proxy flow still writes `sidetrack.toml` by hand, because its "generic" migrations have no command that detection could find.
- ✅ **Git worktrees:** done in PRs #8 and #9. See plan-v1.md, "Git worktrees", decisions 26–30.
  - Each worktree's env file is pointed at its own port automatically (`SetEnvPort`).
  - Removed worktrees free their port, and their database stops counting as in use.
  - Env mode in a worktree only warns, because a containerized app resolves `db` to its own compose network. Proxy mode is the supported setup for worktrees.
- **TLS** (not started, low priority for local dev): the proxy answers "no" to SSL requests, so `sslmode=require` clients fail. Supporting it means terminating TLS in the proxy with a local certificate (e.g. `rustls` with a self-signed cert) and opening TLS upstream when the server needs it.

## Open question for the owner

**Copy speed on the Mac (Docker Desktop).** Run `sidetrack bench` against a real ~2 GB project database and note the time. Linux numbers are in `plan-v1.md`: FILE_COPY took 3.9 s (13 s right after loading), WAL_LOG 11.9 s. The owner plans to run this later (as of 2026-11-04).

## Later, per the spec

- **Tauri desktop app (v1.5).** It's the next big item. The plan:
  - a `crates/sidetrack-app` Tauri 2 crate that calls `sidetrack-core` directly (`Workspace::world`, `Planner`, `Applier`), the same way the CLI does;
  - views from the spec: a git branch map, a DB branch tree (parent → child), container status, commit history per branch, and plan / schema diff previews with Apply;
  - prompts from the git hook could go to the app instead of the terminal ("Tauri approvals").
  - Build it on a desktop machine. It needs a GUI to check, and it's frontend-heavy.
- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
- Approving plans from the desktop app.
