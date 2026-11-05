# Showcase media

Screenshots of the sidetrack desktop app, taken from the real app (not the browser demo data). The app ran against three small demo repositories with real Postgres containers: a Flutter task list, a Next.js storefront and a Tauri notes app. Each has Flyway migrations and feature branches that add their own migration.

All images are 3840×2160: a 1920×1080 window at 2× pixel density.

| File | What it shows |
|---|---|
| `screenshots/01-switch-plan-with-schema-change.png` | All projects. `flutter` has `feature/tags` checked out, so its switch plan is drawn as a proof: repoint `.env`, recreate `api`, run `V2__tags.sql`, the schema change, and disk space. |
| `screenshots/02-new-branch-plan.png` | `nextjs` checked out `feature/checkout`, which has no database yet. The plan copies the closest base and migrates it. |
| `screenshots/03-in-sync.png` | `tauri` is on the right database: the in-sync sheet with its registration mark. |
| `screenshots/04-project-view-branches-and-tree.png` | Project view: git branches on the left, and the DB branch tree (parent → child) with snapshots on the right. |
| `screenshots/05-project-view-commits.png` | A git branch selected: its recent commits in the column, and its database selected in the tree. |
| `screenshots/06-git-branch-gone.png` | A database whose git branch was deleted, marked in the tree. |
| `screenshots/07-destructive-delete-plan.png` | Deleting it: the drop in red, and Apply disabled until the confirmation is ticked. |
| `screenshots/08-delete-confirmed.png` | The same plan with the confirmation ticked. |
| `screenshots/09-reset-to-any-snapshot.png` | Reset: the active database can be replaced with a copy of its parent or any snapshot in the project. |
| `screenshots/10-clean-up-plan.png` | Clean up (`gc`): databases whose git branch is gone, with what dropping them frees. |
| `screenshots/11-collapsed-sidebar.png` | The column folded away (button or Ctrl+B / ⌘B). |
| `screenshots/12-switch-plan-dark.png` | The switch plan in the dark theme. |
| `screenshots/13-project-view-dark.png` | The project view in the dark theme. |
| `screenshots/14-settings-dark.png` | Settings: theme, the step-colour key, the registry. |
| `screenshots/15-applying.png` | A plan applying: the colour bar fills as each step finishes. |
| `screenshots/16-applied.png` | Applied, with each step's time and the sign-off. |

## The two looks

The app has two looks, picked in Settings → Look: **Press Proof** (the files above) and **Canvas** (plain type, node canvases). These are 1440×900 at 1× pixel density.

| Folder | What it shows |
|---|---|
| `screenshots/canvas/real/`, `screenshots/canvas/real-dark/` | Canvas look in the real app against the same three demo repos: each project's Overview (its plan, explained), Branches, Databases, Containers and Structure canvases. Taken before the disk-space fix, so the sidebar shows Docker Desktop's ~1 TB virtual disk instead of the host drive. |
| `screenshots/canvas/light/`, `screenshots/canvas/dark/` | Canvas look on the browser demo data (`npm run dev`), including a database selected with its action card. |
| `screenshots/press-proof/light/`, `screenshots/press-proof/dark/` | Press Proof on the browser demo data, after the Canvas look was added: All projects, the project view, Settings. |
| `screenshots/press-proof/earlier/` | Older Press Proof working captures of the browser demo (applying, applied, in sync, a problem, narrow windows). |

Retake them with `crates/sidetrack-app/scripts/screenshots.mjs` (see the comment at its top).
