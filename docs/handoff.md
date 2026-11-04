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
| Handoff update after PR #10 | merged (PR #11) |
| Desktop app, first surface: All projects, plan proofs, Apply | merged (PR #12) |
| Handoff update after PR #12 | merged (PR #13) |
| Proxy-mode schema summary; the app starts the proxy like the CLI | merged (PR #14) |

`main` is protected. Every PR needs one approval, including a code-owner review (`@Abrar118`). The owner can bypass the rules. Force-pushes and deletion are blocked.

After every PR merge, update this file: the State table, the follow-ups, and the date.

Stale remote branches were deleted on 2026-11-04. Since then, the merged branches `docs/handoff-after-pr10`, `feat/desktop-app`, `docs/handoff-after-pr12` and `feat/app-proxy-autostart` are back on the remote. The owner will clean them up later.

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
- **Restarts:** none. While the proxy runs, a copy of the active database holds the app's new connections in the proxy instead of stopping the app. Commands, the hook and the desktop app start the proxy automatically.

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

## Desktop app (v1.5, in progress)

`crates/sidetrack-app`: Tauri 2, React + Vite + TS. `PRODUCT.md` holds the product context.

**Design:**
- **Visual system:** "Press Proof". Every plan is drawn as a press proof: crop marks, a colour bar with one patch per step, a slug line, and an "OK to apply" sign-off box.
- **Where it's recorded:** the direction contract is in `.impeccable/surfaces/` and the tokens are in `DESIGN.md`. Read both before changing the UI.
- **How it was chosen:** with the Impeccable skill, over two rounds. The first round was rejected as generic.
  - Only the chosen sketch is kept: `.impeccable/mocks/decision/r2-pick.html`. The others were deleted.
  - `.gitignore` keeps future sketches in that folder out of git.

**Built (PR #12):**
- **All projects (home):** every registered project read in parallel, the switch plan for the checked-out branch drawn as a proof, and Apply.
  - **Same plan only:** `apply_switch` builds the plan again and refuses if it differs from the one shown.
  - **Live progress:** the colour bar fills as `Applier` progress lines arrive.
- **States:** in sync, can't read, no projects, settings.
- **Themes:** light and dark.
- **Demo data:** `npm run dev` runs the UI alone with labelled demo data.
- **Finish review:** passed with verdict `ship` after two fix rounds.

**Run against real projects:** the owner ran `npm run tauri dev` against the real registry on 2026-11-04.

**Starts the proxy (PR #14):** before reading a proxy-mode project with `routing.autostart` on, the app starts the proxy the same way the CLI does (`Workspace::proxy_needs_start` / `start_proxy`, now in the core). It runs the `sidetrack` CLI for that, looking next to the app, then on PATH, then in `~/.cargo/bin`. If that fails, the reason shows as a Query on the proof. Not yet checked on a real machine: stop the proxy in a proxy-mode project, focus the app, and the plan should hold connections rather than stop the app.

**Not built yet, in rough order:**
- **Project view:** the two linked panes the user chose, git branches on the left and the DB branch tree (parent → child) on the right, with snapshots, worktrees, container status and commit history per branch.
- **Plans for other operations:** create, snapshot, reset, delete, rename, gc.
  - `Proof.tsx` already handles destructive plans: spot red, a confirm checkbox, the sign-off in spot.
  - Nothing has rendered or reviewed that path yet, because a switch never drops anything.
- **Design ceiling items from the finish review:** the in-sync, problem and settings sheets don't use the proof devices (colour bar, sign-off), and there's no registration target or tint ramp.
- **Approval requests from the git hook** (v2).

**Core gap, fixed in PR #14:** `Workspace::annotate_schema` only looked at `RewriteEnv` steps, so proxy-mode switches never got a schema summary. It now reads `SetProxyTarget` too (`schema_pair`, with a unit test).

## Later, per the spec

- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
