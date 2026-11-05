# Handoff: where things stand and what's next

Last updated 2026-11-05. Read `CLAUDE.md` first, then this.

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
| Handoff update after PR #14 | merged (PR #15) |
| Desktop app: project view (linked git branches and DB branch tree); `Git::log` | merged (PR #16) |
| Desktop app: plans and Apply for create, snapshot, reset, delete, rename, gc | merged (PR #17, via #16's branch) |
| Desktop app: proof devices on sheets with nothing to apply; lead, sign-off and Escape fixes | merged (PR #20; first merged as #18 into an already-merged base, see below) |
| Handoff update after PRs #15–#17 | merged (PR #21) |
| TLS between the app and the proxy | merged (PR #19) |
| Handoff update after PRs #19–#21 | merged (PR #22) |
| Approvals from the git hook: with no TTY, the hook tells the open app, which shows the switch proof | merged (PR #23) |
| Handoff update after PR #23 | merged (PR #24) |
| **Desktop app: reset to any snapshot, delete a snapshot, proxy-start problems on the proof** | **this PR** |

`main` is protected. Every PR needs one approval, including a code-owner review (`@Abrar118`). The owner can bypass the rules. Force-pushes and deletion are blocked.

After every PR merge, update this file: the State table, the follow-ups, and the date.

Stacked PRs: merge the bottom one into `main` first, then retarget the next one to `main` before merging it. Merging an upper PR into its stacked base after that base has merged strands its commits (that's what happened to #18).

Stale remote branches were deleted on 2026-11-04. Since then, the merged branches `docs/handoff-after-pr10`, `feat/desktop-app`, `docs/handoff-after-pr12`, `feat/app-proxy-autostart`, `docs/handoff-after-pr14`, `feat/app-project-view`, `feat/app-plans`, `docs/handoff-after-pr17`, `feat/app-sheet-proofs`, `feat/proxy-tls`, `docs/handoff-after-pr21` and `feat/hook-app-approvals` are back on the remote. The owner will clean them up later.

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
- ✅ **TLS:** done in PR #19. The proxy terminates TLS from the app with a self-signed certificate (`proxy-tls.pem` next to the registry; rustls with `ring`, rcgen). The proxy-to-Postgres leg stays plain. See plan-v1.md, decisions 31–35.
  - Left out: TLS upstream (with TLS on both sides, libpq's `channel_binding=prefer` ties SCRAM to the proxy's certificate and the server rejects the login, so it needs the auth exchange rewritten), and PG17 `sslnegotiation=direct`.

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

**Project view (PR #16):** "Open project" on All projects. Git branches on the left (checked-out first, last 5 commits under the selected one, container status at the foot); the DB branch tree on the right (parent → child, snapshots, which worktree uses each DB, DBs whose git branch is gone or that aren't on the server). Selecting either side selects its partner. Built by a subagent inside the existing Press Proof contract, without a new sketch round.

**Plans for other operations (PR #17):** `ops.rs` adds `plan_op` / `apply_op` keyed by an `Op` enum (switch, create, snapshot, reset, delete, rename, gc), mirroring the CLI's `plan_command`; `apply_switch` is folded in. Actions sit on the selected DB node, on a git branch with no DB (create), and "Clean up" (gc) in the lead. Destructive plans render for the first time: drops in spot, Apply disabled until the plan-worded checkbox is ticked.
- The gaps listed here were closed in the PR after #24:
  - **Reset:** opens a picker with the parent and every snapshot in the project, like `reset --to`.
  - **`snapshot rm`:** each snapshot of the selected node has a Delete.
  - **Proxy-start problems:** if `plan_op` can't start the proxy, the reason is a Query on the proof (`ops::Planned`).

**Sheet devices and fixes (PR #20):** a registration target on in-sync / "nothing to clean up" sheets, a step-colour tint ramp in Settings, the sign-off always reachable on tall proofs, lead and path wrapping, Escape after a failed apply.
- `.impeccable/design.json` is older than `DESIGN.md` and lacks the project view and new devices; `/impeccable document` would refresh it.

**Still to check on a real machine** (owner, `npm run tauri dev`): the project view, including a project with the db container stopped and one with a linked worktree; applying a delete, a reset (including to another branch's snapshot) and a snapshot delete; an `sslmode=require` app through the proxy; with the app open, check out a branch in a GUI git client and approve the switch in the app.

**Dev pitfall:** `npm run tauri dev` loads the UI from `localhost:5173` (`devUrl`). If an older Vite server from another checkout or worktree still holds that port, the app shows that checkout's UI. If that checkout is gone, it shows "Failed to fetch dynamically imported module". Stop the old server first.

**Approvals from the git hook (PR #23):** a checkout with no TTY (a GUI git client) whose plan isn't auto-approved sends the project's root to the app over localhost (`notify.rs`, port in `app.port` next to the registry). The app selects that project, shows its switch proof and flashes the taskbar. The hook doesn't wait, and its line says the plan is waiting in the app. See plan-v1.md, decisions 36–38.
- Checked: `notify::tests`, plus the built app on a temp registry answering `ok` for a registered root and `no` for another.
- Not checked yet: the whole loop from a GUI client into the UI.
- Gaps: while a project is open in the project view, the app only refreshes; if it's the same project, "See the switch plan" appears. A linked worktree's checkout only prints the hint.

**Not built yet:** nothing from the v1.5/v2 app list. Next is MySQL (below), unless the owner's checks turn something up.

**Core gap, fixed in PR #14:** `Workspace::annotate_schema` only looked at `RewriteEnv` steps, so proxy-mode switches never got a schema summary. It now reads `SetProxyTarget` too (`schema_pair`, with a unit test).

## Later, per the spec

- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
