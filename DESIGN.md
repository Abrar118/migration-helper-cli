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

## Do's and Don'ts

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
