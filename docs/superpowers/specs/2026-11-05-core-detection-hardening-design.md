# Core hardening: detection, config and project structure

Status: design, not started. Written 2026-11-05 after PR #34. Pick up from here.

## Why

sidetrack is about to be asked to support more databases. Before that, the owner wants the core to stop being brittle: today, adding a real project often ends in "can't add", and a project that was added can fail later in confusing ways. Adding engines on top of that multiplies the brittleness.

The concrete trigger was Razorbill (a Spring Boot backend). Its dev compose file wrote the JDBC URL directly in `environment:`, setup stopped with "write sidetrack.toml by hand", and the way forward (proxy mode, or a one-line edit) wasn't visible. PR #34 fixed that one case. This design covers the rest of what the analysis found.

## Goals

1. **Fewer "can't add" dead ends.** Detection reports everything it found, each item with where it came from, and leaves only the gaps for the user to fill.
2. **Use what is actually running.** When the stack is up, prefer facts from Docker and Postgres over guesses from files.
3. **Catch config drift early.** A project that was added and later changed (service renamed, variable removed, URL written in directly) is reported plainly, before a plan runs, not as a confusing failure halfway through one.
4. **A measurable baseline.** A set of real project shapes with expected detection results, so every change shows whether it helped.
5. **A config format ready for a second engine**, without building the second engine.

## Non-goals

- Supporting a database other than Postgres. (This work prepares for it.)
- Any AI model, bundled or hosted. Parked; see the handoff. Revisit only if the fixtures show gaps that rules can't close.
- Sidetrack-owned Compose override files, the database rename swap, or moving `sidetrack.toml` outside the repo. (Discussed 2026-11-05, deferred.)
- Dockerfile-only projects without Compose. sidetrack's execution depends on Compose.
- An engine trait. Per plan decision, not until a second engine exists.

## Anti-goals (failure even if it works)

- **sidetrack edits a tracked file.** It may write only untracked or git-ignored files (`.env`, `.git/…`, the registry). Edits to tracked files are suggested for the user to make and commit (decision 44 in `plan-v1.md`).
- **A guess presented as a fact.** Every detected value shows its source; anything uncertain is marked, never silently picked.
- **Parsing compose YAML.** Keep using `docker compose config` (resolved, `--no-env-resolution`, `--no-interpolate`) as today.
- **Running something with side effects to "check" it.** Checks at setup are read-only.
- **A detection change that makes an existing fixture worse.**

## Constraints

- Core logic stays in `sidetrack-core`, with no terminal I/O; the CLI and the app both show its results (`init.rs`'s `Setup::read` is the shared entry).
- Detection stays a pure function of its inputs so it's testable without Docker. Runtime facts are gathered outside it and passed in, the same way compose views are today.
- Existing `sidetrack.toml` files keep working without edits.
- The existing safeguards stay: Add refuses if the config differs from the one shown (decision 39); nothing is written before approval.

## What gets built, in order

### Phase 0: the fixture set (first, before any detection change)

A folder of project shapes under `tests/` (the builder picks the exact path), each with the compose JSON views (the three `docker compose config` outputs), the repo files detection reads, and an expected result. Detection runs on each in a normal `cargo test`.

Shapes to include at minimum:
- Razorbill-like: Spring Boot, JDBC URL in compose `environment:` (list form), `env_file: .env`, non-default compose file name, `db-init/` scripts.
- Django with `DATABASE_URL` in `.env`.
- Rails with `config/database.yml` and `db/migrate`.
- A Prisma monorepo with two apps, one of them also using Alembic.
- A custom Postgres image (`build:` with `FROM postgres:16` in its Dockerfile).
- An app that runs on the host, not in Compose (only the database is in Compose).
- Several Postgres services (one only for `psql`).

The owner was asked which projects they'll add next; their shapes should be added here first. (Not answered yet.)

The expected result records, per field, the value and its source, or "unresolved". A summary test prints how many fields each fixture resolves, so improvements are visible.

### Phase 1: detection returns candidates with evidence

Today `detect()` returns one answer per field and bails at the first missing piece (`detect.rs:104` for the database, `Detection::to_config` for routing and migrations).

Change it to produce, for each field (database service, user, app database name, app services, routing file and variable, migration tool, folders and command):
- the chosen value, or none;
- other candidates;
- the evidence for each: the file and line, the compose service and key, or "seen at runtime";
- a state: found, found with doubt (several candidates), or unresolved.

`Setup::read` no longer fails just because a field is unresolved. The CLI prints the result and says which fields to set; the app shows the result with the unresolved fields to fill in or pick from. `to_config` still refuses to produce a config with required fields missing.

Specific detection gaps to close in this phase:
- **Postgres service.** Today only images whose names contain `postgres`, `postgis`, `timescale` or `supabase/postgres` count (`detect.rs:66,90`). Also count services with `POSTGRES_*` variables, a `pg_isready` healthcheck, container port 5432, or a `build:` whose Dockerfile's `FROM` is one of those images.
- **App services.** Today a service counts only with `depends_on`, or an `@host` in an environment value (`detect.rs:125-131,162-169`). Also match the database host in JDBC URLs (`jdbc:postgresql://host:port/…`, no `@`) and in separate `*_HOST` variables.
- **Compose files.** Today only Compose's default file names are found without the picker. List `*compose*.yml` / `*compose*.yaml` files in the repo as candidates, with the default first.
- **Point to a file.** When no routing variable is found in compose or env files, the user may point at a file; sidetrack shows the connection-like lines in it (`postgres://`, `jdbc:postgresql`, `DATABASE_URL`, `DB_HOST`, `spring.datasource.url`, Django `DATABASES`), and explains what can be routed: an env-style variable in an untracked file (env mode), otherwise the proxy or a suggested edit.

### Phase 2: runtime facts

When the database container is running, gather (read-only) and pass into detection:
- **Which compose files the running stack used.** Compose labels containers with `com.docker.compose.project.config_files` and `com.docker.compose.project.working_dir`. If they differ from the files selected, say so and offer the running ones. (Not yet verified on this machine; nothing was running. Verify before relying on it.)
- **The databases that exist** (`pg_database`), so the app database isn't assumed to be `POSTGRES_DB` when init scripts created another one (`detect.rs:119-122`).
- **Who is connected to which database** (`pg_stat_activity`: database, client address, `application_name`). This identifies the app's database and catches apps that run on the host.

Runtime facts outrank file guesses when they disagree, and the disagreement is shown. With nothing running, detection works from files as today.

### Phase 3: config checks and format

- **One shared config check**, used by `doctor` and before every plan: the database service exists in the current compose config; in env mode, the routing file exists, is untracked, and its variable holds a database name sidetrack can rewrite; restart services exist; migration folders exist. Each failure is one plain sentence saying what to do. Today `doctor` checks git, that the TOML parses, the registry, the hook and the proxy, but nothing against compose.
- **Typed fields.** `database.engine` and `routing.mode` become enums in serde, with the same TOML spelling; unknown values keep a clear error.
- **`version = 1`** at the top of `sidetrack.toml`. A missing version means 1. This gives a second engine a way to migrate old files.

### Phase 4: migrations

Today an if-else chain picks the first tool found (`detect.rs:483-577`), searching four folders deep.
- **Find every tool**, not only the first; when there are several, show them as candidates. Whether sidetrack runs more than one per switch, and what to do with migrations built into the app, are decisions for the owner (see Open questions 3 and 4).
- **More tools:** Spring Boot with Flyway or Liquibase built into the app (the migrations run when the app starts; there may be no CLI command), Liquibase, golang-migrate, sqlx, TypeORM, Drizzle, Ecto, EF Core. For each: how to recognise it, its migration folders, its command, and its version table if `readers.rs` should read it.
- **Read-only command check at setup**, where the tool has one (e.g. `alembic current`, `prisma migrate status`, `flyway info`). Shown as a result, never blocking on its own.

### Before any new engine: the routing-variable decision

Today one variable in one file is rewritten. If several services read the database name from different variables, the others stay on the old database (detection only notes it). This shapes the config format, so the owner decides it before an engine is added (see Open questions).

## Decided

- No AI in this work. (Owner, 2026-11-05.)
- sidetrack writes only untracked files; tracked edits are suggestions. (Owner, 2026-11-05; PR #34.)
- Improve detection, config and structure before more engines. (Owner, 2026-11-05.)
- Order: fixtures, then candidates with evidence and runtime facts, then config checks and format, then migrations; the routing-variable decision before any engine. (Proposed by Claude; the owner asked for this design based on it.)

## Calls made by Claude while writing this (owner to check)

These came up in a builder's read of this document. They fit the decisions above but weren't discussed with the owner.

1. **Doubtful values don't block setup.** When several candidates exist, setup proceeds with the chosen one, shown as "found with doubt" next to the others. Showing it before approval is what keeps it from being silently picked. The CLI with a terminal asks the user to confirm a choice and fill in unresolved fields. Without one, or with `--yes`, it uses the chosen values and fails only on unresolved fields, listing them with the `sidetrack.toml` key for each. The e2e scripts keep working.
2. **Runtime facts preselect; they don't silently override.** When a runtime fact disagrees with the files (for example, the app is connected to a database other than `POSTGRES_DB`), the runtime value becomes the chosen candidate, marked as a disagreement. The user sees both before approving.
3. **Decision 39 still holds, through the user's choices.** The preview records the chosen value for each field, whether picked by the user, preselected from runtime facts or detected. Add sends those choices back. The server reads the files again, applies the same choices and compares the TOML as today. Runtime facts aren't gathered again for the comparison, so a connection that comes or goes between preview and Add can't cause a false refusal. The user still adds exactly what they saw.
4. **The config check before plans.** Checks that would make the plan fail anyway block it: the database service is gone, or in env mode the routing file or variable is gone. Everything else is a warning (`Planned.warnings` in the app, printed in the CLI). In the hook, a block prints the same one-line hint and exits 0, as hook errors do today. To keep `git checkout` fast, the check before plans uses only what planning already gathers plus file reads. The full check against `docker compose config` runs in `doctor`, and when a plan is blocked.
5. **Phase 1 scope.** The candidates cover the fields listed in Phase 1 plus the compose files. The proxy listen address and the `-f` flags added to migration commands keep their current rules (decisions 42 and 43).

## Left to the builder

- The data types for candidates and evidence, and how they serialize to the app.
- The fixture folder layout and expected-result format.
- How the app shows unresolved fields (follow `DESIGN.md` and the Press Proof / Canvas looks).
- Wording of messages, within the convention: plain sentences that say what to do next.
- Splitting `detect.rs` (1,256 lines) into stages if it helps; not required.

## Open questions for the owner

1. Which projects will you add next? Their shapes go into the fixture set first.
2. Routing with several variables: should sidetrack rewrite several variables or files per switch, or keep one and keep warning?
3. Several migration tools in one project: run all of them on a switch (in what order), or ask the user to pick one?
4. **Migrations built into the app** (Spring Boot with Flyway or Liquibase; Razorbill's case): there's no CLI command, and today the config requires `migrations.command` and every switch plan runs it. Options: make the command optional and rely on recreating the app (env mode already does), so the migrations run at its startup; add a plan step that restarts the app and waits until it's healthy; or skip migrations for such projects. This changes the config format, the planner, what happens when migrations fail (today a failed `RunMigrations` leaves the new database selected; see `Outcome::Stopped`), and whether `readers.rs` reads `flyway_schema_history`.
