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

## Test strategy

- **Unit tests:** naming, envfile, config round-trip, compose JSON parsing, detection on fixture directories, file-set logic, planner (pure inputs).
- **Git tests:** temp repos created by the tests themselves.
- **Postgres tests:** run when `SIDETRACK_TEST_PG` is set (a psql command prefix), and are skipped otherwise, so `cargo test` works without Docker.
- **End-to-end:** `tests/e2e/run.sh` creates a compose project (Postgres plus a tiny SQL-file migration runner), a git repo with branches A and B, then runs the spec §1 scenario through the real binary and asserts on the tables that exist.
