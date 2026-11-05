# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`sidetrack` is a Rust CLI that gives every git branch its own local Postgres database, running in Docker Compose. Switching git branches switches which database the app uses, so migration tools never see another branch's migrations. There are two routing modes:
- `env`: rewrite the DB name in an untracked env file and recreate the app containers.
- `proxy`: the app connects through `sidetrack proxy`, which sends it to the active DB branch. Nothing is rolled back; there are no down migrations.

- `docs/spec-v1.md` is the product spec. Requirement IDs like `P0.6` are referenced from code comments.
- `docs/plan-v1.md` is the build plan, plus a "Decisions" list. Read it before changing behaviour: several rules there exist because of bugs.
- `docs/handoff.md` is the current state and what to do next. Start there when resuming work.

## Commands

```bash
cargo build
cargo test                                   # unit tests; Postgres-backed tests skip themselves
cargo test -p sidetrack-core planner::tests::spec_scenario_switch_back_runs_nothing   # one test
cargo fmt --all --check                      # CI runs both of these
cargo clippy --all-targets -- -D warnings

# Include the tests that need a real Postgres (engine + rollback tests):
docker run -d --name st-pg -e POSTGRES_PASSWORD=x postgres:16-alpine
SIDETRACK_TEST_PG="docker exec -i st-pg psql -U postgres" cargo test

# Full scenario in Docker (builds the binary, makes a temp repo from tests/e2e/fixture):
tests/e2e/run.sh
tests/e2e/alembic.sh   # real Alembic; needs `pip install alembic "psycopg[binary]"`
tests/e2e/proxy.sh     # routing.mode = "proxy" with `sidetrack proxy` running

# Desktop app (crates/sidetrack-app):
cd crates/sidetrack-app && npm install
npm run build                                # type-check + build the UI (needed before clippy)
cargo clippy -p sidetrack-app --all-targets -- -D warnings
npm run tauri dev                            # run the app against the real registry
npm run dev                                  # UI only, in a browser, with demo data

# Running the CLI by hand against some project:
SIDETRACK_REGISTRY=/tmp/st.sqlite cargo run -p sidetrack -- -C /path/to/project status
```

`SIDETRACK_REGISTRY` points the registry somewhere other than `~/.local/share/sidetrack/registry.sqlite`. Use it for manual testing so the real registry stays clean. The e2e script needs a running Docker daemon, and `script` (util-linux) for its terminal-prompt step.

## Architecture

Three crates:
- **`crates/sidetrack-core`** holds all logic and does no terminal I/O. That includes `init.rs` (`Setup::read` / `register`, shared by `sidetrack init` and the app's Add project) and `hook.rs` (installing the git hook).
- **`crates/sidetrack`** is a thin layer: clap commands (`main.rs`), prompts and approval (`ui.rs`), and the git hook (`hook.rs`).
- **`crates/sidetrack-app`** is the Tauri 2 desktop app (v1.5).
  - **Rust side:** `src/main.rs` (commands), `src/views.rs` (per-project summary, and `project_detail` for the project view) `src/ops.rs` (`Op`: switch, create, snapshot, reset, delete, rename, gc, each planned the way the CLI's `plan_command` does) and `src/add.rs` (Add project: `preview`, `start_database`, `add`). It drives the core like the CLI does.
  - **Project view** (`ui/src/ProjectPage.tsx`): git branches in the column (with `Git::log` commits), the DB branch tree on the sheet, linked by selection. Without Docker or the database it still shows the registry and git, with server facts unknown, and offers no actions.
  - **Actions:** on the selected DB node (snapshot, and reset to its parent or any snapshot in the project, only for the active DB; delete only for an inactive one; nothing for another worktree's), on each snapshot of the selected node (delete, `snapshot rm`), on a git row with no DB (create), and Clean up (gc) in the lead. `plan_op` returns the plan, plus any proxy-start problem as `warnings` (`ops::Planned`), drawn by the same `Proof` as switches; the planner's refusals show as a Query where the action was chosen.
  - **Frontend:** React + Vite + TS in `ui/`.
  - **Add project** (`ui/src/AddProject.tsx`): a folder picker (`tauri-plugin-dialog`), then what `init` found as a proof with an "OK to add" sign-off. `add_project` refuses if the TOML differs from the one shown, and the database must be running (the sheet can start it). It offers a Compose file picker and guided proxy setup for inline URLs (including JDBC). Selected files reach preview, Start, Add and detected migration commands. Start and Add both re-read the shown configuration. New proxies used by containers listen on `0.0.0.0:54320`; the UI requires acknowledgment of the one-time connection change and a published database port before Add.
  - **Not a default workspace member:** it needs WebKitGTK and a built frontend, so build it with `cargo build -p sidetrack-app`. CI checks it in its own job.
  - **Same plan only:** `apply_op` builds the plan for the same `Op` again and refuses if it differs from the one shown, or if it drops a database without `confirm_destructive`.
  - **Visual system:** "Press Proof". Read `.impeccable/surfaces/*.md` (the direction contract) and `DESIGN.md` before changing the UI.
  - **Second look, "Canvas"** (Settings → Look): a sidebar of each project's sections and node canvases (`ui/src/Canvas.tsx`, `ProjectCanvas.tsx`, `canvas.css`, all scoped to `[data-look="canvas"]`). Shared parts word themselves for it via `usePlain()` (`look.ts`); both looks share project state through `useProject` (`project.ts`). See `DESIGN.md` → "The Canvas look".
  - **Demo data:** `npm run dev` runs the UI alone with labelled demo data (`ui/src/demo.ts`).

Every command follows the same pipeline:

1. **`workspace::Workspace`** loads `sidetrack.toml`, the git repo, compose, the Postgres engine and the registry. `world()` then gathers everything into a `planner::World`: registry rows, the databases that actually exist on the server, open connections, running services, whether the env file is tracked, and the **active DB**.
2. **`planner::Planner`** turns `World` + `Config` + `Git` into a `plan::Plan` and changes nothing. Planner tests build `World` by hand on temporary git repos (`git::testutil`).
3. **`ui::run_plan`** prints the plan and gets approval: `--yes`, the per-repo "always" flag (stored in the registry, never for destructive plans), or a `/dev/tty` prompt.
4. **`apply::Applier`** is the only code that mutates anything. It runs the steps in order and keeps an undo stack.
   - If a step fails, completed steps are undone in reverse.
   - `DropDatabase` is a barrier: undo can't pass it, so destructive orderings clone first and drop last (see `Planner::reset`).
   - A failure in `RunMigrations` or `RunSeed` deliberately does not roll back. The new database stays selected for debugging (`Outcome::Stopped`).

### Where the truth lives

- **Active database:** whatever the env file's variable currently points at, or in proxy mode the target file `.git/sidetrack-proxy-target` (`Workspace::active_db`). Never the registry.
- **Which databases exist:** the Postgres server. The SQLite registry (`registry.rs`) is only a record. `Planner::reconcile` reports mismatches as warnings and never auto-fixes them.

### Choosing a base for a new branch (`migrations.rs`, `Planner::auto_base`)

sidetrack never parses migrations. It compares sets of migration file names, read with `git ls-tree` from `migrations.dirs`.

Each `Candidate` has two sets. **Keep them separate:**
- `files` ("may contain": recorded ∪ files at its git branch tip) decides compatibility. A candidate is compatible when `files ⊆ target`.
- `applied` ("known applied": recorded only) decides which migrations to list as pending.

Merging the two once caused needed migrations to be skipped (plan decision 9).

### Version-table readers (`readers.rs`)

When the migration tool has a version table (`alembic_version`, `django_migrations`, …, or `migrations.version_table`), `World.readings` holds what each DB branch really has applied, and `Planner::db_state` prefers it over the registry's records. Unknown rows become `?unknown/<row>` entries in the "may contain" set, so that database is never compatible.

- **Alembic** keeps only heads. The `Catalog` maps revision → (file, down revisions), parsed from files across all local branches.
- **`Postgres::version_rows`** feeds psql a script on stdin (`\connect` / `\gset` / `\if`) rather than `-c`, so a missing table isn't an error.
- **Frozen snapshots** refuse connections, so they're never read.

### Postgres engine (`engine/postgres.rs`)

- No driver: everything runs through a psql command prefix, by default `docker compose exec -T <service> psql -U <user>`. The override is `database.psql` in config, or `SIDETRACK_TEST_PG` in tests.
- Copies are `CREATE DATABASE … TEMPLATE` (`STRATEGY FILE_COPY` on 15+, `file_copy_method = clone` tried first on 18). They fail if anything is connected to the source. So when the source is the active DB, the planner adds `StopServices` first and `clone_database` terminates leftover sessions and retries.
- There's only one engine. Per the plan, don't introduce a shared engine trait until a second engine (MySQL) exists.

### Detection (`detect.rs`, used by `sidetrack init`)

Never parse compose YAML. Detection combines three outputs of `docker compose config --format json` (`compose::ConfigView`):
- **resolved:** final values.
- **`--no-env-resolution`:** keeps the `env_file` paths, so you can tell which file defines a variable.
- **`--no-interpolate`:** keeps `${VAR}` references, so you can follow them into the project `.env`.

Detection is a pure function of those models plus the repo files, so tests feed it JSON fixtures.

### Proxy (`proxy.rs`, `routing.mode = "proxy"`)

- Blocking std threads, no async runtime.
- It rewrites `database` in the startup message only when it equals `routing.database`, and leaves other names alone.
- After that it only relays bytes, so authentication passes through untouched.
- **TLS:** an SSL request gets "S" and the proxy ends TLS itself (rustls with `ring`), with a self-signed certificate kept next to the registry (`proxy-tls.pem`, `proxy::tls_config`). The rewrite then happens on the decrypted stream. The connection to Postgres is always plain (plan decision 34). If the certificate can't be loaded, SSL requests get "N" as before.
- A watcher polls the target file, and when the target changes it closes the app's connections to the old database.
- Plans use `Step::SetProxyTarget` instead of `RewriteEnv`.
- When the proxy is running, copies use `HoldConnections` / `ReleaseConnections` (`Planner::push_pause` / `push_unpause`) instead of stopping the app. The hold file sits next to the target and expires.
- Without a running proxy, plans stop the app and start only the services that were stopped.
- The CLI, the hook and the desktop app start the proxy before planning (`Workspace::proxy_needs_start` / `start_proxy`, `routing.autostart`). `start_proxy` runs the sidetrack binary it's given: the CLI passes itself, and the app looks for `sidetrack` next to itself, on PATH, then in `~/.cargo/bin`.
- `sidetrack proxy --daemon` re-runs the binary in the background (pid and log in `.git/`); `--stop` kills it. `init --proxy` writes proxy-mode config (`RoutingConfig::use_proxy`).
- Tests use a fake upstream (unit) and `tests/e2e/proxy.sh` (real Docker).

### Git worktrees

- `Workspace.main_root` is the main worktree. The registry key (`root_key`) and `db_compose` (database service, psql, port, df) use it. `compose` is this checkout's own, for app services.
- Per worktree, under `git_path`: proxy target, pid, log, and `sidetrack-proxy-listen` (the next free port after `routing.listen`, saved once). The hold file is in the common git dir (`Workspace::proxy_hold_path`).
- `World.elsewhere` (db → worktree) protects other worktrees' databases. `delete` and `rename` refuse them, `gc` keeps them, and copies hold or warn.
- In a linked worktree, `switch` starts with `Step::SetEnvPort` (`Planner::push_worktree_env`, `World.worktree_env`). It points `routing.file`'s URL at the worktree's proxy port, copying the main worktree's env file first if there's none.

### Naming (`naming.rs`)

- Git branch → `<sanitized>_db`. If that's over 63 bytes (the Postgres limit), or the name collides with another branch, it becomes `<truncated>_<6-hex sha256>_db`.
- Snapshots are `<db>__s<seq>`; reset uses a temporary `<db>__tmp`.

### Git hook (`crates/sidetrack/src/hook.rs`)

- `hook run` must always exit 0 and must never prompt without a TTY. Without one, it prints a one-line hint.
- It intentionally does **not** skip when old HEAD == new HEAD, because `git checkout -b` needs a new DB branch.
- The installed block sits between `# >>> sidetrack >>>` markers, right after the shebang.
- Without a TTY, it also tells the desktop app, if it's open (`notify.rs`). That's localhost TCP, with the port in `app.port` next to the registry. Only the project root is sent, and only from the main worktree. The app shows that project's switch proof, and the hook doesn't wait for an answer.

## Adding a plan step

To add a `plan::Step` variant, touch all of these:
- `Step::describe`, and `is_bookkeeping` / `is_destructive` if they apply (`plan.rs`).
- `Applier::run`, which returns the step's `Undo` (`apply.rs`).
- The exhaustive `kinds()` helper in the planner tests.

Plans serialize to JSON (`--json`) as `{"type": "<snake_case>", …}`, and that format is meant to stay stable for the future desktop app. An applied plan's `Report` carries per-step `timings`; `bench` reads its numbers from there.

Things added after planning, outside the pure planner: `Workspace::annotate_schema` (schema summary on switch plans) and `World.disk` (from `df` inside the DB container), which `Planner::guard_disk` checks for every plan that copies.

## Conventions

- User-facing messages are plain sentences that say what to do next (e.g. "start the database first: `docker compose up -d db`").
- Commit messages use `feat(core):`, `feat(cli):`, `docs:`, `chore:` prefixes.
- `main` is protected: every PR needs 1 approval, including a code-owner review (`.github/CODEOWNERS` makes `@Abrar118` the owner of every file). Force-pushes and deleting the branch are blocked. Only the repo owner can bypass the rules (admin bypass is on) to merge or push directly. Everyone else works on a branch or fork and opens a PR.
