---
name: sidetrack
description: A press proof for database plans. Check it, sign it off, then it runs.
colors:
  light-table: "#ededed"
  paper: "#ffffff"
  key-black: "#1a1a1a"
  soft-ink: "#5e5e5e"
  rule-grey: "#d4d4d4"
  process-cyan: "#00a0e0"
  process-magenta: "#e5007d"
  process-yellow: "#ffe600"
  spot-red: "#e10600"
  on-light-ink: "#1a1a1a"
  on-strong-ink: "#ffffff"
  light-table-night: "#121212"
  paper-night: "#1c1c1c"
  key-night: "#f0f0ee"
  soft-ink-night: "#a6a6a6"
  rule-night: "#333333"
typography:
  figure:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "56px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.03em"
  display:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "44px"
    fontWeight: 700
    lineHeight: 1.05
    letterSpacing: "-0.025em"
  patch-numeral:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "44px"
    fontWeight: 700
    lineHeight: 0.9
    letterSpacing: "-0.02em"
  column-figure:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "40px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "32px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.02em"
  row-title:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: "-0.015em"
  title:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 600
    letterSpacing: "-0.01em"
  step:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 400
  body:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.45
    fontFeature: "tnum"
  label:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
  note:
    fontFamily: "Familjen Grotesk, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
  identifier:
    fontFamily: "Courier Prime, ui-monospace, monospace"
    fontSize: "0.94em"
    fontWeight: 400
  slug:
    fontFamily: "Courier Prime, ui-monospace, monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.6
rounded:
  none: "0px"
spacing:
  hairline: "2px"
  xs: "6px"
  sm: "10px"
  md: "18px"
  lg: "28px"
  xl: "40px"
  sheet: "44px"
components:
  button-primary:
    backgroundColor: "{colors.key-black}"
    textColor: "{colors.paper}"
    typography: "{typography.step}"
    rounded: "{rounded.none}"
    padding: "0 18px"
    height: "46px"
  button-primary-hover:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.key-black}"
  button-primary-drop:
    backgroundColor: "{colors.spot-red}"
    textColor: "{colors.on-strong-ink}"
    rounded: "{rounded.none}"
    height: "46px"
  button-secondary:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.key-black}"
    rounded: "{rounded.none}"
    padding: "0 18px"
    height: "46px"
  button-secondary-hover:
    backgroundColor: "{colors.light-table}"
  text-button:
    textColor: "{colors.key-black}"
    typography: "{typography.label}"
    padding: "0"
  project-row:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.key-black}"
    padding: "18px 28px 20px"
  project-row-hover:
    backgroundColor: "{colors.light-table}"
  project-row-selected:
    backgroundColor: "{colors.key-black}"
    textColor: "{colors.paper}"
  proof-sheet:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.none}"
    padding: "30px 44px 0"
  colour-bar:
    height: "78px"
  step-number:
    typography: "{typography.title}"
    size: "34px"
  signoff:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.key-black}"
    padding: "16px 20px 18px"
    width: "330px"
  slug-line:
    textColor: "{colors.key-black}"
    typography: "{typography.slug}"
  registration-target:
    textColor: "{colors.key-black}"
    size: "45px"
  tint-ramp:
    size: "34px"
---

# Design System: sidetrack

## Overview

The app has two looks, picked in Settings → Look and stored per viewer (`sidetrack.look`): **Press Proof** (the default, described first and in the tokens above) and **Canvas** (plain type, node canvases; see "The Canvas look" below). Both draw the same data and the same plans, and both need an Apply before anything runs. Light and dark apply to each.

**Creative North Star: "The Press Proof"**

Every plan is a prepress proof laid on a light table. It sits inside thin crop marks, opens with a colour bar of numbered patches, lists its steps, states the facts (schema change, disk before and after), carries a typed slug line, and ends in an "OK to apply" box. Nothing runs until the proof is signed. The world refuses the category default of sidebar, list, detail and a blue Apply button in a drawer.

The system is paper and key black on a grey table. Colour is never decoration: the three process inks name step families and the spot red names a drop. Type is one grotesk, set big and tight for numerals and titles and plain for sentences, with a typewriter face kept for the things a person would copy: database names, commands and the slug. The dark theme is the same light table at night: the ground and the sheet go dark, the key ink inverts, and the process and spot inks keep their values and roles.

Density is low and the scale is large. Rows carry a 24px name and one plain sentence of state. Figures run at 40 to 56px. Labels are sentence-case soft text above their values.

**Key Characteristics:**
- Paper sheets on a light-table ground, flat, square-cornered, framed by 1px crop marks.
- A colour bar whose patches are the plan's steps, each in its family's ink.
- One grotesk (Familjen Grotesk) with Courier Prime only for identifiers and slugs.
- Hairline 1px rules between rows and steps; 2px key borders on the sign-off box and its buttons.
- A registration target on a sheet with nothing to sign off, when everything is in register.
- Motion is ink laid down left to right with a clip-path wipe.

## Colors

Neutral paper and key black carry the interface; four printing inks carry meaning and nothing else.

### Primary
- **Key Black** (light `key-black`, dark `key-night`): the default ink. Text, the Apply button, the selected project row, crop marks, the slug rule, the sign-off border, the focus ring and the disk meter fill. Key-family step patches (services and connections) use it too. In dark mode it inverts to near-white, with near-black text on it.

### Secondary
- **Process Cyan** (`process-cyan`): database copy, create, freeze and rename steps. Text on it is `on-light-ink` in both themes.
- **Process Magenta** (`process-magenta`): routing steps: env rewrite, proxy target, worktree port. Text on it is `on-strong-ink`.
- **Process Yellow** (`process-yellow`): migrations and seed. Text on it is `on-light-ink`. Yellow is also the text selection colour.

### Tertiary
- **Spot Red** (`spot-red`): drops only. Used on drop patches and step numbers, the border of a destructive sign-off box, its Apply button, and the accent of the "can't be undone" checkbox.

### Neutral
- **Light Table** (light `light-table`, dark `light-table-night`): the app ground behind sheets, row and secondary-button hover, the empty part of the disk meter.
- **Paper** (light `paper`, dark `paper-night`): the left column, proof sheets (Settings is one too) and the sign-off footer.
- **Soft Ink** (light `soft-ink`, dark `soft-ink-night`): labels, notes, pending steps, row state sentences, inactive nav.
- **Rule Grey** (light `rule-grey`, dark `rule-night`): 1px hairlines between rows and steps, the column edge, the scrollbar and disabled button borders.

### Named Rules
**The Process Ink Rule.** Cyan, magenta and yellow appear only as step families: patch fills, patch tints and step-number squares. Never use them for status, links, emphasis or decoration.

**The Spot Rule.** Spot red marks a drop and nothing else. A plan that drops a database turns its sign-off box and Apply red; a plan that drops nothing has no red on the sheet.

**The Same Inks At Night Rule.** The dark theme changes only the table, paper, key, soft and rule values. The process and spot inks keep the same values and roles.

## Typography

**Display Font:** Familjen Grotesk (with system-ui, sans-serif)
**Body Font:** Familjen Grotesk (weights 400, 500, 600, 700 bundled)
**Label/Mono Font:** Courier Prime (with ui-monospace, monospace)

**Character:** A tight, confident grotesk sets big numerals and titles; a typewriter face appears only where something should be copied exactly. Numerals are tabular everywhere.

### Hierarchy
- **Figure** (700, 56px, line-height 1, -0.03em): the disk fact on a proof, "41.3 → 40.1" with a 22px "GB free".
- **Display** (700, 44px, 1.05, -0.025em, balanced wrap): sheet titles ("Switch billing to fix/invoice-rounding") and the Settings title. 36px in short windows.
- **Patch numeral** (700, 44px, 0.9): the step number in a colour-bar patch. 36px below 1280px wide or 820px tall.
- **Column figure** (700, 40px, -0.03em): disk free at the foot of the column, with an 18px unit. 34px in short windows.
- **Headline** (700, 32px, line-height 1): "All projects".
- **Row title** (600, 24px, 1.15): project names in the column.
- **Title** (600, 22px, -0.01em): the lead line above a sheet, fact values, the sign-off heading (700), problem text (400).
- **Step** (400, 17px): step sentences, button labels (600), theme names.
- **Body** (400, 16px, 1.45): the base.
- **Label** (600, 15px, soft): fact and setting labels such as "Schema change" and "Disk on the Docker host", sentence case. Plain 15px soft for row states, notes and step meta.
- **Note** (400, 14px, soft): the records line and the demo notice.
- **Identifier** (Courier Prime 400, 0.94em): database names, branch names on the route line, commands in backticks, the registry path.
- **Slug** (Courier Prime 400, 13px, 1.6): the slug line; 14px when it stands alone.

### Named Rules
**The Courier Rule.** Courier Prime is only for identifiers (database names, commands, paths) and the slug line. Titles, sentences, labels and figures stay in the grotesk.

**The Sentence-Case Rule.** No tracked caps labels and no eyebrows. A label is a 15px semibold soft sentence placed directly above its value.

## Layout

Two columns: a 360px paper column (320px below 1280px wide) and a light-table main area that scrolls. The column has a 28px inset throughout. The main area pads 34px on top and `clamp(32px, 4.5vw, 64px)` on the sides, with no bottom padding so the sign-off can reach the window edge.

The lead line above a sheet shares a row with its side (path · mode, then text buttons) only when the sentence fits on one line beside it; otherwise the side drops below, right-aligned, and the sentence takes the full width. Its last name or word is tied to the one before it with a no-break space, and it wraps `pretty` where supported, so a database name or a last word never sits alone on a line. Side items are kept whole and break only after a " · ".

A proof sheet fills at least `100vh - 150px`, so the sign-off footer sits at the sheet's foot. The sheet body pads 30px 44px (26px 32px below 1280px). Facts sit in two equal columns with a 40px gap and stack below 1280px. The sheet footer is always sticky at the window's bottom edge: on a proof taller than the window, the slug and sign-off stay in reach and a 1px rule-grey line marks where the footer starts over the steps; when the proof fits, or once scrolled to its end, it sits in place with no rule. In windows under 820px tall, vertical spacing tightens (row padding 13/15px, colour bar 66px, footer 14/20px).

Rhythm: 2px gaps between patches and between the two squares of a tint ramp; 6 and 10px inside groups; 18 to 22px between blocks; 28px column inset; 40 to 44px for sheet padding and fact gaps. Prose is capped at 75ch.

## Elevation & Depth

Flat. Depth is paper on a table: the sheet is a lighter (or, at night, raised-grey) plane on the ground, framed by crop marks, with no drop shadows. Separation comes from 1px rules and from tone, not from shadow.

### Named Rules
**The Flat Paper Rule.** No drop shadows and no blur. A sheet is distinguished from the table by its tone and its crop marks.

## Shapes

Every corner is square; no radius is declared anywhere. Borders are either 1px hairlines (rules, the slug rule in key) or 2px key strokes (sign-off box, primary and secondary buttons, the active nav underline). Crop marks are 22px by 1px key strokes set 8px out from each sheet corner. Step numbers are 34px solid squares.

## Components

### Buttons
- **Shape:** square, 46px tall, 2px border, 18px side padding, 17px semibold.
- **Primary (Apply, Done):** key fill with paper text. Hover swaps to paper fill with key text (160ms ease-out).
- **Primary on a drop plan:** Apply only: spot fill and border with white text; hover swaps to paper with spot text. Disabled until the "can't be undone" box is ticked, whose label says what goes in the plan's words ("Delete feature_x_db and its 2 snapshots."). Done and Close after it stay key.
- **Disabled:** paper fill, soft text, rule-grey border.
- **Secondary (Not now, Close):** paper fill, key text and border; hover takes the table tone.
- **Text button (Refresh):** 15px semibold key text, 1px underline offset 4px; disabled loses the underline and goes soft.
- **Focus:** a 2px key outline offset 3px everywhere; on the selected row it turns paper and sits 6px inside.

### Navigation
Projects and Settings as plain 15px text in soft ink. Hover goes key; the current page is key, 600, with a 2px key underline.

### Project rows
Full-width buttons on paper, separated by 1px rules: a 24px semibold name and one 15px soft sentence of state ("feature/refunds will use feature_refunds_db · plan ready"). A row that can't be read sets its state in key at 600. Hover takes the table tone. The selected row is solid key with paper text, inverted at night. Arrow keys move the selection.

### Project view: git rows and the DB branch tree
The column becomes the project: "Back to all projects", the project name as the headline, then git branch rows (the name in Courier at 19px, one soft sentence of state; "No database yet" in key at 600). The selected row is solid key and opens its last commits underneath: subject on one line, Courier id and age below, between 1px rules. The column foot lists containers: a 10px key square, filled when running, empty when stopped, rule-grey when unknown, with "Stopped" in key at 600.

The sheet holds the DB branch tree: nodes with the Courier name at about 20px, size on the right, and one 15px soft sentence where facts that need a look ("the app uses it", "used in <worktree>", "its git branch is gone", "not on the server") are set in key at 600. Children hang off 1px key rules with a 12px tick, drawn like crop marks; separate roots are split by a 1px rule. Snapshots are 14px soft lines under their branch. Selection is shared: a git row and its DB node are selected together, both solid key; arrow keys move through either list. No process inks: they stay for step families.

### Actions and inline fields
Actions sit under the selected DB node, on paper, as 16px underlined text buttons in a row ("Snapshot", "Reset to app_dev", "Rename", "Delete"), closed by a 1px key rule; a soft 15px sentence says why an action is missing ("The app uses it, so it can't be deleted"). An action that needs input opens a small field in place, never a dialog: a 15px semibold soft label over a 46px square input with a 2px key border (Courier for branch names), then a secondary "Show the plan" and a "Cancel" text button. In the column, a git branch with no database gets the same field with a select ("Create its database from"). The planner's refusals appear under the field as a Query. Choosing an action replaces the tree with its proof; Escape steps back one level (proof, field, project).

### Proof sheet with crop marks
A paper sheet with four corner crop marks, holding in order: colour bar, title, step list, records line, Query notes, facts, then the footer with the slug line and sign-off box.

Sheets with nothing to apply (in sync, can't read, no projects, load error, Settings, the project's DB branch tree, "Nothing to clean up") use the same paper and crop marks without the colour bar or the sign-off box: the bar is a plan's steps and the box asks for an OK, so a sheet with no plan carries neither. Each uses only the devices that say something true about it:
- **In sync** and **Nothing to clean up:** the registration target beside the title (everything is in register).
- **Can't read** (a project, the project view, the registry): the problem set as a 22px Query, since it needs an answer before anything else.
- **No projects:** a standalone slug, "0 projects · <registry path>", saying where sidetrack looked.
- **Settings:** the step colours tint ramp.

### Registration target
A 45px target in key at the top right of a sheet's title: 1px crosshair and 15px-radius circle, a 7px solid centre with the cross knocked out in paper. On a press sheet it shows the plates line up; here it means sidetrack checked and everything is in register: the checked-out branch and the database the app uses ("in sync"), or every DB branch and its git branch ("Nothing to clean up"). It appears only after a successful check, never on a proof (a plan means something is out of register) and never on a sheet that couldn't be read. Hidden from screen readers; the title says the same.

### Step colours (tint ramp)
In Settings, the key to the colour bar: one row per family between 1px rules, each with two 34px squares (the family's full ink, then its 16% waiting tint mixed with the theme's paper) and a 17px line naming the family and its steps ("Cyan Copy, create, freeze or rename a database"). Full ink is a step that has run, or will once you apply; the tint is a step still waiting while a plan applies. The ramp is the only place the inks appear outside a proof, and only as the key to it.

### Colour bar and patch states
One patch per visible step, flexed equally with 2px gaps, 78px tall. Each patch shows its numeral bottom-left and a one-word label ("Copy", "Route", "Migrate") bottom-right. The patch takes its ink from the step family:
- **key:** stop or restart services; hold, release or close connections.
- **cyan:** copy, create, freeze, rename a database.
- **magenta:** repoint env, route the proxy, set the worktree port.
- **yellow:** migrate, seed.
- **spot:** drop a database or its temporary copy.

States: **inked** (full ink, text in the ink's foreground); **pending** (a 16% tint of the ink on paper, key text); **running** (the ink wiping in from the left); **failed** (paper with a 3px inset ring). In review every patch is inked; while applying, earlier patches are inked, the current one runs and later ones are pending. Bookkeeping steps get no patch, only a quiet records line.

### Step list
Numbered rows at least 48px high between 1px rules: a 34px ink square holding the 22px bold numeral, the step sentence at 17px with database names in Courier, and soft meta on the right (copy size, or the time once applied). Pending steps set their sentence in soft ink.

### Facts
Two columns, each a 15px soft label over a value: the database the app will use (Courier, 22px) or the schema change as a 18px semibold list ("+ table refunds", "− column payments.captured", up to four lines), and the disk figure at 56px with "→" between before and after.

### Query notes
Plan warnings start with a bold inline "Query", as on a proofreader's query, followed by the message at 15px with commands in Courier. Notes without a query are 15px soft. Both are capped at 75ch.

### Slug line
A Courier 13px line under a 1px key rule, items joined with " · " and each item kept unbroken: project, plan kind, step count, bytes added or "no copy", "nothing destructive" or "drops N databases", and "applied HH:MM" once signed.

### Sign-off box
A 330px box with a 2px key border (spot on drop plans) holding a 22px bold heading and its actions. It records the proof's life in place: "OK to apply" with Apply and Not now; "Applying" with "Step n of N"; then "Applied", "Not applied" or "Stopped" with the result and the next action. Once the plan has run or failed, Escape does what that action's Close (or Done) does; while it applies, Escape does nothing. The box appears only on a proof: a sheet with nothing to apply never gets a sign-off.

### Motion: the ink wipe
Patch ink is a full-bleed layer revealed with `clip-path: inset(0 100% 0 0)` → `inset(0 0 0 0)` over 700ms ease-out, as if laid down left to right. A running patch animates toward 78% coverage over 9s and holds there until the step finishes, then completes. Patch text changes colour over 300ms. Under `prefers-reduced-motion: reduce`, transitions on patches and buttons are removed and a running patch sits still at half coverage (`inset(0 50% 0 0)`), so progress still reads without movement.

## The Canvas look

For people who find Press Proof's big type and inks too much. Plain system type at normal sizes, calm neutrals, and each project shown as node canvases you can drag around. Built in `ui/src/canvas.css` (everything scoped to `:root[data-look="canvas"]`), `Canvas.tsx` (the canvas engine) and `ProjectCanvas.tsx` (the sections). Shared parts such as the plan read `usePlain()` (`look.ts`) to switch wording.

### Tokens
- **Type:** the system stack (`-apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans`) at 13px/1.5; page titles 18px 600; small text 12px. Code and database names in `ui-monospace` (SF Mono, Cascadia Mono, Menlo, Consolas) at 12px. No display sizes; the biggest number on a plan is 22px.
- **Neutrals (light / dark):** ground `#f6f8fa` / `#0d1117`, panel `#ffffff` / `#151b23`, ink `#1f2328` / `#e6edf3`, soft `#59636e` / `#9198a1`, rule `#d1d9e0` / `#3d444d`, hover `#eef1f4` / `#212830`.
- **Roles:** accent blue `#0969da` / `#4493f8` for selection, links and focus; green `#1a7f37` / `#3fb950` for running and Apply; amber `#9a6700` / `#d29922` for "worth knowing"; red `#cf222e` / `#f85149` for stopped, deletes and a drop plan's Apply. Each has a soft tint for badges and callouts.
- **Node types (light / dark),** shown as a 7% tint and a half-strength border: git `#8250df` / `#a371f7`, database `#0969da` / `#4493f8`, snapshot `#1b7c83` / `#39c5cf`, image `#bc4c00` / `#f0883e`, source folder `#9a6700` / `#d29922`, container `#bf3989` / `#db61a2`, database container `#0550ae` / `#79c0ff`, volume `#1a7f37` / `#3fb950`, mounted folder `#6e7781` / `#9198a1`, `sidetrack.toml` `#57606a` / `#b1bac4`, routing `#cf6a00` / `#ffa657`, migrations `#7d4e00` / `#e3b341`.
- **Shape:** 6px radius on controls, 8px on nodes, cards and panels; 1px borders; one soft shadow (`0 8px 24px` at 12%) only on things that float (the node card, the dock, the key).

### Layout
- **Sidebar** (240px, folds to a 48px strip with its toggle or Ctrl+B): Projects, each with a state dot (green in sync, amber plan waiting, red can't read). The selected project opens its sections under it: Overview, Branches, Databases (with a count), Containers, Structure. Add project, then Settings and disk space at the foot. Nothing in it navigates while a plan applies.
- **Pane:** a header (18px title, the lead sentence in soft text, then path · mode and buttons: See the switch plan, Clean up, Refresh), then the section.
- **Overview** is the project's switch plan, or its in-sync or problem sheet. The other sections are canvases.

### Canvases
- **Branches:** git branches (left) linked to their databases (right) by "uses" edges. A branch with no database has a dashed border and a "no database yet" badge.
- **Databases:** the DB tree left to right ("copied to" edges), snapshots stacked under their database with dashed borders and dashed edges.
- **Containers:** images and source folders → containers → volumes and mounted folders, plus a Databases node. Edges say "runs as", "built into", "mounts", "holds" and "reads <variable>". "needs" (depends_on) edges appear only while one of their services is selected. Paths inside the project are shown as `./sub/dir`.
- **Structure:** repo → `sidetrack.toml` → compose, routing, migrations and the database → app services, migration folders and the database the app uses.
- **Every canvas:** drag a node to move it (saved per project and section); drag the background or scroll to pan; Ctrl+scroll to zoom; Fit and Reset layout bottom right. A graph wider than the pane fits itself on first view, leaving room for the key. Edge labels sit on the curve's midpoint with a halo in the ground colour, and turn blue with the edge when an end is selected.
- **Key:** a card pinned top right, listing only the types on that canvas; "Hide key" folds it (remembered).

### Acting on a canvas
- **Node card:** clicking a node opens a card under it with that node's actions (the same fields as Press Proof: snapshot label, reset source, rename, create from). A second click, a click on empty canvas, or Escape closes it.
- **Dock:** drop targets float bottom centre: Snapshot / Reset the app's database to it / Delete on Databases, Create from closest match / Create empty on Branches. While dragging, targets that take the node turn blue (red for Delete), the rest grey out, and the hint names the plan ("Drop to plan: delete old_checkout_db"). Dropping a git branch on a database copies it for that branch; a snapshot or the parent onto the app's database resets it.
- **A drop only plans.** It opens the plan in place of the canvas; nothing runs until Apply.
- **Details panel** (bottom, 180px, hideable and remembered): Details, Commits, Warnings tabs for the selected node.

### A plan in the Canvas look
No colour bar, crop marks or slug. In order: the title; a now → after pair of boxes ("The app uses now" → "After you apply", or the create, reset or delete equivalent) with one sentence on why; "What happens when you apply" over the numbered steps, each with a coloured kind chip (Copy, Repoint, Restart, Migrate, Drop…); records line; a "Worth knowing" amber callout for warnings and notes; "Tables and columns the app will see" with added / not there badges and what is compared with what; free disk space with a sentence on the change; then a plain summary ("4 steps · copies 7.3 MB · nothing is deleted") and the sign-off ("Ready to apply / Nothing runs until you click Apply", or "Ready to apply, and it deletes data" with the red Apply and the confirm box).

### Canvas rules
- **Do** keep type at 12 to 18px; size never carries hierarchy past the page title.
- **Do** colour a node only by its type, and name every type used on a canvas in its key.
- **Do** label every edge with its relation, in two words or fewer.
- **Do** keep every drag action available from the node card too, for keyboard users.
- **Don't** reuse Press Proof's devices (crop marks, colour bar, slug, registration target, process inks) in this look.
- **Don't** use colour on a node for state; state goes in a badge or a dot.

## Do's and Don'ts

These are Press Proof's rules; the Canvas look has its own above.

### Do:
- **Do** show every operation as a proof with an "OK to apply" sign-off before anything runs.
- **Do** colour a step by its family (key, cyan, magenta, yellow, spot) and use that ink for both its patch and its number square.
- **Do** set database names, commands, paths and the slug in Courier Prime at 0.94em of the surrounding size.
- **Do** keep corners square, rules 1px, and sign-off strokes 2px key.
- **Do** keep the process and spot inks identical in light and dark; switch only the table, paper, key, soft and rule values.
- **Do** keep the reduced-motion fallback: a static half-inked running patch, no transitions.
- **Do** give a sheet a device only when it says something true about that sheet: the registration target only after a check passed, the tint ramp only as the key to the colour bar.

### Don't:
- **Don't** use cyan, magenta or yellow for anything other than a step family.
- **Don't** use spot red for anything but a drop.
- **Don't** use Courier for titles, labels, sentences or figures.
- **Don't** set labels in tracked caps or add eyebrows above titles; labels are sentence-case 15px semibold soft text.
- **Don't** add drop shadows, rounded corners or a coloured Apply button outside a drop plan.
- **Don't** put a colour bar or a sign-off box on a sheet with no plan; an empty bar or a box with nothing to sign is decoration.
