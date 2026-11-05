# Handoff: where things stand and what's next

Last updated 2026-11-05. Read `CLAUDE.md` first, then this.

## Compose setup fixes (merged PR #32)

- Compose environment parsing accepts maps and `KEY=value` lists, preserving embedded `=`, empty strings and bare names.
- Add project offers a Compose file picker and guided proxy routing for inline JDBC URLs. The CLI accepts repeated `init -f` / `--compose-file` flags. Selected files are persisted and used by generated Compose migration commands, Start and Add; both actions recheck the reviewed settings.
- New proxies use loopback on macOS/Windows; Linux app containers select `0.0.0.0:54320`. Non-loopback listeners show a network-access warning in init and Add project, because only Postgres authentication and the firewall control remote access. A disposable-container probe reached host loopback on macOS OrbStack; Docker Desktop/macOS and Windows remain unverified. Add requires a published database port (or explicit upstream) and acknowledgment of the one-time app connection change. Add saves the setup and starts the proxy when autostart is enabled; update the app URL and recreate containers afterward.
- Review follow-ups: migration-command qualification and absolute-path portability are explained in Add project; non-Compose commands stay unchanged. CLI relative `-f` uses the current directory or `-C`. The picker can append, reorder and remove override files; proxy hints use the configured port. Upstream lookup is shared, and fresh detection/review has regression coverage.
- Second review: the host-OS/local-Docker listener assumption and remote-daemon limitation are explicit in decision 42 and the README. A pure listener policy tests all four host/container combinations. Absolute-path warnings now come from one core helper and appear in both CLI and app, including CLI `--yes`; paths remain supported. No bridge discovery or unrelated minor changes were added.
- Razorbill's `docker-compose-dev.yml` passes a read-only proxy setup preview; no Razorbill files or databases were modified. Its generated `flyway migrate` command still needs validation: the app bundles Flyway in Spring Boot, rather than providing a standalone Flyway executable. No real switch was applied there.
- Verification: 93 workspace tests (88 core, 2 CLI integration, 3 app) with a dedicated Postgres container, both Clippy checks, formatting, UI/native builds; browser smoke checks of ordered override selection/reordering/removal, migration-command notices, custom proxy ports, proxy confirmation, database start, rereview and Add in Press Proof/light and Canvas/dark. Native file-dialog interaction and real Razorbill Apply remain untested.

## Inline compose URL fallback (branch fix/inline-url-suggest)

- When the database URL is written directly in compose `environment:` (Razorbill's dev file), env-mode setup no longer stops at "write sidetrack.toml by hand". It names the one-line edit: the new value only (`<url with ${DB_NAME:-<current db>}>`), so it fits both `KEY=value` and `KEY: value` compose files, plus `DB_NAME=<current db>` in the untracked `.env`. If `.env` sets that name to another database, any value in Compose's raw output references it (`environment:`, `command:`, `healthcheck:`, top-level `secrets:`, …), or it's exported in sidetrack's own environment (an export overrides `.env`, so later switches wouldn't take effect), it tries `SIDETRACK_DB_NAME`, then `SIDETRACK_DB_NAME_2`, and so on. Limit: the desktop app started from Finder doesn't see variables exported in the user's shell profile; the CLI does. The default keeps the current database wherever the variable isn't set. CLI init and Add project show the same message; Add project's error sheet has **Check again**.
- Decided: sidetrack only writes untracked files. Tracked edits are suggested for the user to make and commit. Sidetrack-owned Compose overrides, a rename swap, proxy changes, a `pg_stat_activity` drift check and AI discovery are all out of scope for now.
- Verified against Razorbill read-only (`init -f docker-compose-dev.yml` prints the edit; no files written). The owner will fix Razorbill's dev compose, which serves as the end-to-end check.

## AI-assisted configuration discovery (parked)

- Discussed 2026-11-05: a bundled small model (Qwen3.5-0.8B card shows LongBench v2 26.1 vs 38.7 for 2B; Qwen2.5-Coder-1.5B Q4_K_M is 1.12 GB) is a poor fit for a one-time setup step with a few output fields. The preferred order is setup-flow fixes, deterministic candidate search plus "point to the file", runtime evidence from `pg_stat_activity`, and, optionally, handing off to the user's own coding agent. A model is only worth revisiting if a benchmark against the non-AI baseline shows a gap. No AI design or model choice is approved, and no model has been bundled, trained or benchmarked.
- The published Tiny Recursive Model (TRM) was considered; its demonstrated puzzle tasks do not establish repository/configuration discovery capability. A small pretrained code-language model was suggested as an alternative to evaluate, not selected. Any future discovery output still needs deterministic validation and the existing plan/review/apply safeguards.
- Separately deferred: keeping project files untouched through Sidetrack-owned external Compose overrides and external configuration storage. This is not implemented; current Sidetrack still expects sidetrack.toml inside the repository. Do not fold this into the AI work without discussing scope.
- Remaining review items from PR #32 were intentionally left outside the second follow-up: localhost false-positive exposure warnings, exposure warnings remaining advisory, missing standalone-proxy/post-add network notices, upstream-normalization test coverage, duplicate port extraction and duplicate override entries.
- Local testing uses OrbStack on macOS. The throwaway project is `../sidetrack-playground`, with main, feature/orders and feature/notes, its own registry, and install-cli.sh / run-app.sh / start.sh. Razorbill files and databases were not changed; its Spring Boot/Flyway migration command remains unvalidated before real Apply.

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
| Desktop app: reset to any snapshot, delete a snapshot, proxy-start problems on the proof | merged (PR #25) |
| Handoff update after PR #25 | merged (PR #26) |
| Desktop app: Add project (init from the app); init and hook install in the core | merged (PR #27) |
| Desktop app: collapsible column; a plain error when a project's folder is gone | merged (PR #28, through #27's branch) |
| Showcase screenshots in `public/`; Vite pre-bundles the Tauri modules | merged (PR #29) |
| Desktop app: a second look, "Canvas" (sidebar of sections, node canvases, drag and drop); host-drive disk space under Docker Desktop | merged (PR #30) |
| Handoff update after PR #30 | merged (PR #31) |
| Compose environment formats, explicit Compose files, guided proxy setup and review fixes | merged (PR #32) |

`main` is protected. Every PR needs one approval, including a code-owner review (`@Abrar118`). The owner can bypass the rules. Force-pushes and deletion are blocked.

After every PR merge, update this file: the State table, the follow-ups, and the date.

Stacked PRs: merge the bottom one into `main` first, then retarget the next one to `main` before merging it. Merging an upper PR into its stacked base after that base has merged strands its commits (that's what happened to #18). Commits pushed to a PR after it merged are stranded too (that happened to #28's last commit).

Stale remote branches were deleted on 2026-11-04. Since then, the merged branches `docs/handoff-after-pr10`, `feat/desktop-app`, `docs/handoff-after-pr12`, `feat/app-proxy-autostart`, `docs/handoff-after-pr14`, `feat/app-project-view`, `feat/app-plans`, `docs/handoff-after-pr17`, `feat/app-sheet-proofs`, `feat/proxy-tls`, `docs/handoff-after-pr21`, `feat/hook-app-approvals`, `docs/handoff-after-pr23`, `feat/app-op-gaps`, `docs/handoff-after-pr25`, `feat/app-add-project`, `feat/app-collapsible-sidebar`, `docs/showcase-and-handoff-after-pr28` and `feat/app-canvas-look` are back on the remote. The owner will clean them up later.

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

## How the owner likes to work

These were kept in one Claude account's memory; they're here so any session has them.
- **Follow the owner's stated UI structure.** For the Canvas look, the owner cut off Impeccable's themed-direction cards and dictated the layout. Use plain system type at normal sizes; skip concept-seed and decision-page rounds unless asked. When asked to design "with Impeccable", show one or two quick HTML sketches of the owner's own structure and get a yes before building.
- **After a PR merges** ("merged"): pull `main` first, then refresh this file through its own PR.
- **Screenshots go in `public/screenshots/`** and are listed in `public/README.md`. `.impeccable/review/` is git-ignored scratch space.
- Short answers, real verification (screenshots of the actual render, real Docker when possible), and say plainly what wasn't checked.

## The Canvas look (PR #30)

A second look, picked in Settings → Look; Press Proof stays the default and is unchanged. `DESIGN.md` → "The Canvas look" has the tokens and rules; the chosen layout sketch is `.impeccable/mocks/decision/canvas-d.html` (a full canvas, a card of actions under the selected node, a dock of drop targets, a details panel at the bottom).

**Built:**
- **Shell:** collapsible 240px sidebar (Ctrl+B): projects with a state dot, the selected project's sections (Overview, Branches, Databases, Containers, Structure), Add project, Settings, disk space.
- **Overview:** the project's switch plan, explained for this look in `Proof.tsx` behind `usePlain()` (`look.ts`): now → after boxes with a sentence on why, "What happens when you apply" with a kind chip per step, a "Worth knowing" callout, schema rows with added / not there badges, disk with a sentence, a plain summary and "Ready to apply".
- **Canvases** (`Canvas.tsx` engine, `ProjectCanvas.tsx` sections): pan, Ctrl+scroll zoom, Fit, Reset layout, draggable nodes (positions saved per project and section in localStorage), auto-fit when wider than the pane, colour-coded node types with a hideable key, a label on every edge.
  - Branches: git branches → their databases. Databases: the DB tree with snapshots. Containers: images and source folders → containers → volumes, folders and databases ("needs" edges only for the selected service). Structure: repo → `sidetrack.toml` → compose, routing, migrations, database → services, folders, the active DB.
- **Acting:** a card of actions under the clicked node (the same fields as Press Proof), closed by a second click, a click on the canvas, or Escape. A dock of drop targets: drag a git branch with no DB onto a database to copy it; a snapshot or the parent onto the app's database to reset; the app's database to Snapshot; anything else to Delete. A drop only opens the plan.
- **Details panel:** Details / Commits / Warnings for the selected node; hideable and remembered.
- **Shared refactor:** the project view's state moved to `project.ts` (`useProject`), used by both looks.
- **Backend:** `ProjectDetail.services` now lists every compose service with image, build context, `depends_on` and volumes (from `docker compose config`); `ProjectDetail.structure` summarises `sidetrack.toml`.
- **Disk space under Docker Desktop (core):** `df` inside the container reports Docker Desktop's sparse ~1 TB virtual disk. `Workspace::disk_space` now also reads the host drive holding that disk (`docker_desktop_dir`, `host_disk`: `GetDiskFreeSpaceExW` on Windows, `df` elsewhere) and keeps the smaller (`limit_by_host`, unit-tested). `DiskSpace.host_limited` says so; `doctor`, the disk guard and both looks use it. Native Linux Docker is unchanged.

**Checked:** fmt, clippy (workspace and app), all tests, the UI build; the Impeccable detector on the changed files; screenshots of every section in light and dark, in the browser demo and in the real app against `D:\Demo-showcase` (real Docker and Postgres, nothing applied); `sidetrack doctor` there reads 77.4 GB free of 291.8 GB (drive C:) instead of 950 GB. The owner checked drag and drop by hand.

**Left to do:**
- Retake `public/screenshots/canvas/real*` now that disk space is fixed (`node crates/sidetrack-app/scripts/screenshots.mjs app <dir>` while the app runs with WebView2 remote debugging; see the script's header).
- The macOS Docker Desktop path in `desktop_disk_dirs` (`~/Library/Containers/com.docker.docker/Data/vms/0/data`) is untested; Colima and OrbStack aren't detected.
- No Apply has been run from the Canvas look against a real database yet (the plans and Apply are the same code as Press Proof's).
- Containers gets busy with many services; the dock can cover the lowest nodes until you pan or Fit. Arrow keys don't move between canvas nodes (Tab does, and every drag action is also in the node card).
- Impeccable's finish review wasn't run for this look, and `.impeccable/design.json` predates both looks' latest changes (`/impeccable document` refreshes it). No surface brief was written for the Canvas look; `DESIGN.md` is its record.
- `D:\Demo-showcase` (the three demo repos and their registry) lives only on the owner's old machine.

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
- The gaps listed here are closed in PR #25:
  - **Reset:** opens a picker with the parent and every snapshot in the project, like `reset --to`.
  - **`snapshot rm`:** each snapshot of the selected node has a Delete.
  - **Proxy-start problems:** if `plan_op` can't start the proxy, the reason is a Query on the proof (`ops::Planned`).

**Sheet devices and fixes (PR #20):** a registration target on in-sync / "nothing to clean up" sheets, a step-colour tint ramp in Settings, the sign-off always reachable on tall proofs, lead and path wrapping, Escape after a failed apply.
- `.impeccable/design.json` is older than `DESIGN.md` and lacks the project view and new devices; `/impeccable document` would refresh it.

**Still to check on a real machine** (owner, `npm run tauri dev`): the project view, including a project with the db container stopped and one with a linked worktree; applying a delete, a reset (including to another branch's snapshot) and a snapshot delete; an `sslmode=require` app through the proxy; with the app open, check out a branch in a GUI git client and approve the switch in the app; add the `D:\Demo` projects with Add project.

**Dev pitfall:** `npm run tauri dev` loads the UI from `localhost:5173` (`devUrl`). If an older Vite server from another checkout or worktree still holds that port, the app shows that checkout's UI. If that checkout is gone, it shows "Failed to fetch dynamically imported module". Stop the old server first.

**Approvals from the git hook (PR #23):** a checkout with no TTY (a GUI git client) whose plan isn't auto-approved sends the project's root to the app over localhost (`notify.rs`, port in `app.port` next to the registry). The app selects that project, shows its switch proof and flashes the taskbar. The hook doesn't wait, and its line says the plan is waiting in the app. See plan-v1.md, decisions 36–38.
- Checked: `notify::tests`, plus the built app on a temp registry answering `ok` for a registered root and `no` for another.
- Not checked yet: the whole loop from a GUI client into the UI.
- Gaps: while a project is open in the project view, the app only refreshes; if it's the same project, "See the switch plan" appears. A linked worktree's checkout only prints the hint.

**Add project (PR #27):** "Add project" in the column (and on the empty screen) picks a folder and shows what `init` found as a proof: database, routing variable, migrations, checked-out branch, and the `sidetrack.toml` it writes. It can start the database, and the git hook is opt-out. See plan-v1.md, decisions 39–41.
- **Moved to the core:** `init::Setup` and `sidetrack_core::hook`. The CLI's `init` now checks that the database is running before writing `sidetrack.toml`.
- **Also fixed:**
  - A switch that only runs migrations used to read "should switch to" with no name, and "The app will use ?". It now reads as migrations to run.
  - A hook checkout that arrived during a refresh was lost; the app now reads again.
- **Checked end to end:** on a clone of the flutter demo, with a temp registry, in the real app:
  - preview, then start the database, then add, with the hook installed;
  - the first plan, migrations only (Flyway V1), applied;
  - a `git checkout feature/tags` from a shell with no TTY reached the app, and that switch was applied (copy, repoint, V2);
  - checking out `main` again brought up the switch back.
- **Demo repos:** `D:\Demo\{flutter,nextjs,tauri}` on the owner's machine (not in this repo) are for trying this.
  - Each has Postgres, a stand-in `api` and Flyway migrations, with `main` plus two feature branches that each add a `V2`.
  - The generator script isn't committed.

**Showcase screenshots (PR #29):** `public/screenshots/` has 16 full-screen PNGs (3840×2160) of the real app, indexed in `public/README.md`. They were taken against `D:\Demo-showcase` on the owner's machine, which has its own registry. Add project isn't among them, because its native folder dialog can't be driven from the WebView.

**Collapsible column (PR #28):** the toggle before the wordmark, or Ctrl+B (⌘B), folds the column to a 56px rail in both views. It isn't triggered while typing in a field, and the choice is remembered in localStorage (`sidetrack.sidebar-collapsed`).

**Not built yet:** nothing from the v1.5/v2 app list. Next is MySQL (below), unless the owner's checks turn something up.

**Core gap, fixed in PR #14:** `Workspace::annotate_schema` only looked at `RewriteEnv` steps, so proxy-mode switches never got a schema summary. It now reads `SetProxyTarget` too (`schema_pair`, with a unit test).

## Later, per the spec

- MySQL/MariaDB engine. That's when a shared engine trait gets introduced, not before. The proxy would need a MySQL handshake rewrite too, since the database name is in the HandshakeResponse.
- Then SQL Server, Oracle and others, one at a time.
