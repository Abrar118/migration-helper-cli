# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`sidetrack` is a Rust CLI that gives every git branch its own local Postgres database, running in Docker Compose. Switching git branches switches which database the app uses (by rewriting the DB name in an untracked env file and recreating the app containers), so migration tools never see another branch's migrations. Nothing is rolled back; there are no down migrations.

- `docs/spec-v1.md` is the product spec. Requirement IDs like `P0.6` are referenced from code comments.
- `docs/plan-v1.md` is the build plan, plus a "Decisions" list. Read it before changing behaviour: several rules there exist because of bugs.
- `docs/handoff.md` is the current state and what to do next (the v2 local proxy design). Start there when resuming work.

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

# Running the CLI by hand against some project:
SIDETRACK_REGISTRY=/tmp/st.sqlite cargo run -p sidetrack -- -C /path/to/project status
```

`SIDETRACK_REGISTRY` points the registry somewhere other than `~/.local/share/sidetrack/registry.sqlite`. Use it for manual testing so the real registry stays clean. The e2e script needs a running Docker daemon, and `script` (util-linux) for its terminal-prompt step.

## Architecture

Two crates:
- **`crates/sidetrack-core`** holds all logic and does no terminal I/O. A planned Tauri desktop app (v1.5) will reuse it.
- **`crates/sidetrack`** is a thin layer: clap commands (`main.rs`), prompts and approval (`ui.rs`), and the git hook (`hook.rs`).

Every command follows the same pipeline:

1. **`workspace::Workspace`** loads `sidetrack.toml`, the git repo, compose, the Postgres engine and the registry. `world()` then gathers everything into a `planner::World`: registry rows, the databases that actually exist on the server, open connections, running services, whether the env file is tracked, and the **active DB**.
2. **`planner::Planner`** turns `World` + `Config` + `Git` into a `plan::Plan` and changes nothing. Planner tests build `World` by hand on temporary git repos (`git::testutil`).
3. **`ui::run_plan`** prints the plan and gets approval: `--yes`, the per-repo "always" flag (stored in the registry, never for destructive plans), or a `/dev/tty` prompt.
4. **`apply::Applier`** is the only code that mutates anything. It runs the steps in order and keeps an undo stack.
   - If a step fails, completed steps are undone in reverse.
   - `DropDatabase` is a barrier: undo can't pass it, so destructive orderings clone first and drop last (see `Planner::reset`).
   - A failure in `RunMigrations` or `RunSeed` deliberately does not roll back. The new database stays selected for debugging (`Outcome::Stopped`).

### Where the truth lives

- **Active database:** whatever the env file's variable currently points at (`Workspace::active_db`), not the registry.
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

### Naming (`naming.rs`)

- Git branch → `<sanitized>_db`. If that's over 63 bytes (the Postgres limit), or the name collides with another branch, it becomes `<truncated>_<6-hex sha256>_db`.
- Snapshots are `<db>__s<seq>`; reset uses a temporary `<db>__tmp`.

### Git hook (`crates/sidetrack/src/hook.rs`)

- `hook run` must always exit 0 and must never prompt without a TTY. Without one, it prints a one-line hint.
- It intentionally does **not** skip when old HEAD == new HEAD, because `git checkout -b` needs a new DB branch.
- The installed block sits between `# >>> sidetrack >>>` markers, right after the shebang.

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
