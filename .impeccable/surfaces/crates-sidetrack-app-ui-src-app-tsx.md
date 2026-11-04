---
version: 1
slug: "crates-sidetrack-app-ui-src-app-tsx"
primary_target: "crates/sidetrack-app/ui/src/App.tsx"
related_targets: []
---

## Scope

The sidetrack desktop app (Tauri 2, React + Vite + TS), first surface: **All projects** (home). Operate mode: people open it for a task, then close it. Later surfaces in the same world: project view (git branches | DB branch tree, linked), a plan proof for every operation, applying.

## Audience and task

The backend developer who wants an overview of every project, and the teammate who avoids the CLI. Task: see whether each project's database matches its checked-out branch, read what a plan will do, and apply it. All data comes from `sidetrack-core` (registry, `Workspace::world`, `Planner`, `Applier`); the plan shown is the plan applied.

## Direction contract

THESIS: A plan is a proof: you check it, sign it off, and only then does anything run. It refuses the category default of sidebar, list, detail and a blue Apply button in a drawer.

OWN-WORLD: Paper white #FFFFFF and key black #1A1A1A on a light-table grey #EDEDED. Process cyan #00A0E0, magenta #E5007D and yellow #FFE600 only as step families. Spot red #E10600 only for drops. Familjen Grotesk for the interface, Courier Prime for database names and the slug line. Thin black crop marks. Dark theme: the light table at night, with a #121212 ground, a #1C1C1C proof sheet and the same inks in the same roles.

STORY: The visitor sees every project's branch and database state in one column, understands the open plan as a proof, and signs it off with Apply.

FIRST VIEWPORT:
- **Left column (360px, paper):** the wordmark with Projects and Settings, an "All projects" heading at about 32px, and project rows (name at 24px plus one plain sentence of state). The selected row is solid key black. Disk free sits at the bottom in 40px numerals with a bar.
- **Main area (light-table ground):** a status line, then the proof sheet inside crop marks:
  - a colour bar of numbered step patches, about 78px tall
  - the title at 40px
  - the numbered step list
  - the schema change, and disk "41.3 → 40.1" at 52px
  - a Courier slug line
  - an "OK to apply" box holding Apply (key black) and Cancel

The signature interaction: while a plan runs, its patches fill in order like ink laying down, and the sign-off box records the result.

FORM: Press Proof (prepress proofs and the OK-to-print sign-off), position 1 on my ordered list for the round-2 hand. seed:44402d99fcc9a018

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Step families (colour bar)

- **Key:** stop or start services; hold, release or terminate connections.
- **Cyan:** clone, create, freeze (snapshot) or rename a database.
- **Magenta:** env rewrite, proxy target, worktree port.
- **Yellow:** migrations, seed.
- **Spot red:** drop database.
- **Bookkeeping steps:** no patch; a quiet "records" line.

## Unresolved

- Native Windows support (spec open question 6).
- Approval requests from the git hook (v2).
