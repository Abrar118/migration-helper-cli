# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(A Tauri 2 desktop app: the UI is a web view. Which desktop OSes it targets follows the CLI: Linux and macOS. Native Windows is spec open question 6.)

## Stack

- Tauri 2 shell, frontend in **React + Vite + TypeScript** (the user's choice, 2026-11-04).
- The Rust side calls `sidetrack-core` directly (`Workspace::world`, `Planner`, `Applier`), the same way the CLI does. No shelling out to the CLI.
- The crate lives at `crates/sidetrack-app` (decided 2026-11-04).
- Light and dark themes, both first-class (decided 2026-11-04).

## Users

Two users, served equally (confirmed 2026-11-04):

- **A backend developer who already uses the CLI.** They juggle several feature branches and run Postgres and the app in docker-compose, with migrations often running inside the app container. They open the app for an overview: every project in the global registry, which DB branch each git branch has, what's running, and what's drifted. They want it dense and fast to scan.
- **A teammate who avoids the CLI.** They want to switch, snapshot, reset and clean up without a terminal. For them the app is the main way they use sidetrack, so plans have to be understandable without knowing the internals.

## Product Purpose

sidetrack gives every git branch its own local Postgres database in Docker Compose. Switching git branches switches which database the app uses, so migration tools never see another branch's migrations, and nothing is ever rolled back. The desktop app makes that state visible (git branches, DB branches as a parent → child tree, containers, commits) and lets people preview and apply the same plans the CLI runs.

Success: someone can see at a glance whether their database matches their branch, understand what a switch will do before it happens, and do it from the app.

## Positioning

Database branches, not migration rollbacks: each branch gets a real copy (`CREATE DATABASE … TEMPLATE`), so down migrations are never needed. It works with any migration tool, running in local Docker, and handles git branches it has never seen by picking the nearest compatible base. Every change is shown as a plan before it runs.

## Operating Context

- Local development only: a docker-compose project with a Postgres service, a git repo, and an app service that reads the DB name from an env file (env mode) or connects through `sidetrack proxy` (proxy mode).
- Used next to the terminal, an editor and git GUIs. Branch switches also happen outside the app (`git switch`, IDEs), and the git hook reacts to them, so the app has to reflect changes it didn't make.
- One global registry (SQLite) holds every project on the machine, so the app can show all of them.
- Git worktrees: in proxy mode, several DB branches can be active at once, one per worktree, each on its own proxy port.

## Capabilities and Constraints

- **Plan, then apply.** Every operation builds a `Plan` first (steps, costs, warnings, notes). Only the `Applier` changes anything. Plans serialize to JSON as `{"type": "<snake_case>", …}`, and that format is meant to stay stable for this app.
- **Approval rules match the CLI.** Destructive plans are always confirmed. The per-repo "always approve" flag never applies to destructive plans.
- **Failure behaviour:** if a step fails, completed steps are undone in reverse. `DropDatabase` is a barrier undo can't pass. A failed migration or seed deliberately does not roll back: the new database stays selected for debugging (`Outcome::Stopped`).
- **Where the truth lives:** the active DB is whatever the env file (or the proxy target) says, and which databases exist is whatever the Postgres server says. The registry is only a record; mismatches are shown as warnings and never fixed automatically.
- **v1.5 scope (confirmed 2026-11-04):** maps (git branches, DB branch tree, containers, commit history per branch), plan previews with a schema summary, and Apply. Approval requests sent from the git hook to the app are v2.
- **Available operations:** switch, create, snapshot, reset, delete, rename, gc, status, doctor, bench. Proxy start/stop (`--daemon` is Unix-only).
- **Terminology:** *DB branch* (a live, writable copy for a git branch), *snapshot* (a frozen copy kept to restore from, `<db>__s<seq>`), *base* (the database a new branch is copied from), *adopted* (an existing database linked at `init`), *active DB*, *pending migrations*, *holding connections* (the proxy pauses new connections during a copy), *elsewhere* (a database in use by another worktree).
- Only Postgres for now; MySQL/MariaDB is next.

## Brand Commitments

- Name: **sidetrack**, lowercase.
- Voice: user-facing messages are plain sentences that say what to do next, e.g. "start the database first: `docker compose up -d db`". Never surprise the user.

## Evidence on Hand

- `README.md`, `docs/spec-v1.md` (product spec), `docs/plan-v1.md` (build plan and numbered decisions), `docs/handoff.md` (current state).
- Real benchmark (Linux, in plan-v1.md): FILE_COPY 3.9 s (13 s right after loading), WAL_LOG 11.9 s for a ~2 GB database. The macOS number hasn't been measured yet; don't invent one.
- No logo, screenshots, users, or testimonials exist. Don't fabricate any.

## Product Principles

1. **Never surprise.** Nothing changes without a plan the user has seen. Destructive steps are always called out and confirmed.
2. **Show reality, not records.** Display what the server, the env file and git say. When the registry disagrees, show the mismatch and explain it; never hide or auto-fix it.
3. **The plan is the product.** The same plan drives the CLI, the hook and the app. The app renders and applies plans; it doesn't invent its own operations.
4. **Fast round trip.** Switching to a branch that already has a database should feel instant, and anything slow (copies, migrations) shows its progress.
5. **Explain without dumbing down.** Dense enough for the CLI user and clear enough for the teammate who avoids it.
