# Handoff: where things stand and what's next

Last updated 2026-11-03. Read `CLAUDE.md` first, then this.

## State

| What | Where |
|---|---|
| v1 CLI (Postgres, `.env` routing, hook, snapshots, gc, …) | merged to `main` (PR #2) |
| v1.1: bench, disk guard, schema summary, rename, snapshot retention | merged to `main` (PR #3) |
| Version-table readers (`readers.rs`, `tests/e2e/alembic.sh`) | **PR #4, branch `feat/migration-readers`, open.** Lint, 57 unit tests and both e2e scripts passed. Merge it before starting new work. |

`main` is protected: every change goes through a PR (admins included). No approvals are required, and force-pushes and deletion are blocked.

Stale remote branches that can be deleted once PR #4 is merged: `docs/v1-spec`, `feat/v1-cli`, `docs/claude-md-branch-rules`, `feat/v1.1`, `claude/serene-volta-pj0vti`.

## Getting going on a new machine

```bash
git clone git@github.com:Abrar118/migration-helper-cli.git && cd migration-helper-cli
git checkout main && git pull            # after PR #4 is merged
cargo test                               # Postgres tests skip without SIDETRACK_TEST_PG
docker run -d --name st-pg -e POSTGRES_PASSWORD=x postgres:16-alpine
SIDETRACK_TEST_PG="docker exec -i st-pg psql -U postgres" cargo test
tests/e2e/run.sh
python3 -m venv .venv && .venv/bin/pip install alembic "psycopg[binary]"
PATH="$PWD/.venv/bin:$PATH" tests/e2e/alembic.sh
```

On macOS, `tests/e2e/run.sh` uses `script` for its terminal-prompt step, with util-linux syntax (`script -qec`). BSD `script` takes different flags, so that one step may need a tweak on a Mac.

## Open question for the owner

**Copy speed on the Mac (Docker Desktop).** Run `sidetrack bench` against a real ~2 GB project database and note the time. Linux numbers are in `plan-v1.md`: FILE_COPY took 3.9 s (13 s right after loading), WAL_LOG 11.9 s. If the Mac is much slower, consider Postgres 18 `file_copy_method = clone` and the proxy work below.

## Next: local proxy (v2, spec P2)

Goal: the app always connects to its usual database name (e.g. `app_dev`) through a local proxy, and the proxy sends the connection to the active DB branch. Switching then needs no `.env` rewrite and no container restart.

### Design sketch, not yet started

1. **A `Router` abstraction in core.** Today routing is `.env` rewrite + `StopServices` / `RecreateServices` steps chosen by the planner. Add `routing.mode = "proxy"` (the config already reserves it; `Config::validate` rejects it for now).
   - **`env` router:** today's behaviour, unchanged.
   - **`proxy` router:** a switch writes the active DB into a state the proxy reads, either a registry row or a small file under `~/.local/share/sidetrack/`. It plans a new step (e.g. `SetProxyTarget`) instead of `RewriteEnv` + recreate.
   - Follow "Adding a plan step" in `CLAUDE.md`.
   - **Active DB:** today `Workspace::active_db` reads the env file. In proxy mode it must read the proxy target instead. That's a key decision (plan decision 2).
2. **The proxy process:** `sidetrack proxy` (foreground) or `sidetrack proxy --daemon`. It listens on a local port (e.g. 54320) and connects upstream to the DB container's published port.
   - Upstream address: from compose (`docker compose port db 5432`) or config.
   - Likely a new crate `crates/sidetrack-proxy`, or a module in the CLI using tokio. Core stays sync, with no tokio.
3. **The Postgres wire protocol:**
   - The client's first message is an `SSLRequest` (8 bytes, code 80877103), a `GSSENCRequest` (80877104), or a `StartupMessage` (protocol 196608) carrying `user`, `database` and options as NUL-terminated pairs.
   - To SSL / GSS requests, answer `N` (no SSL), then read the real StartupMessage. With `sslmode=prefer` (the libpq default) clients continue in plain text; with `sslmode=require` they fail with a clear error. Document this, since local dev rarely uses TLS.
   - Rewrite `database` (if it equals the configured "app name", or always) to the active DB branch, recompute the length, send the result upstream, then copy bytes both ways (`tokio::io::copy_bidirectional`). Authentication passes through untouched, so no secrets are handled.
   - A `CancelRequest` (80877102) arrives on a new connection. Forward it to the same upstream as is: the backend key data identifies the session.
4. **Switching while connections are open:** new connections go to the new DB. Existing ones keep the old DB until they reconnect. Options: (a) terminate sessions on the old DB through the engine (pool reconnects), or (b) leave them. Start with (a), opt-in via config.
5. **Copies still need no connections to the source.** In proxy mode, instead of `StopServices`, the proxy can briefly hold new connections to the source and the engine terminates existing ones. Simplest first version: keep `StopServices` for the clone step only.
6. **Tests:**
   - Unit-test startup-message parsing and rewriting (pure bytes).
   - Integration: start the proxy against `SIDETRACK_TEST_PG`'s server, then `psql -h 127.0.0.1 -p <port> -d app_dev -c 'select current_database()'` returns the branch DB.
   - e2e: a variant of `run.sh` with `mode = "proxy"`, the `api` service pointing at `host.docker.internal:54320`.
7. **Spec items to settle while building:**
   - **Getting containers to the host proxy:** `host.docker.internal` on Mac, `extra_hosts: host-gateway` on Linux. Alternatively, run the proxy as a compose service.
   - **Process lifecycle:** pidfile, a `doctor` check that the proxy is up, and the hook behaviour when it isn't.

## Later, per the spec

- Tauri desktop app (v1.5), which reuses `sidetrack-core` and the stable `--json` plan format.
- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before.
- Then SQL Server, Oracle and others, one at a time.
- Git worktrees, and approving plans from the desktop app.
