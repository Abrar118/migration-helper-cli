import { useEffect, useRef, useState, type ReactNode } from "react";
import { applyOp } from "./api";
import { bytes, gb, plural, seconds } from "./format";
import { addedBytes, command, createsDatabase, describe, ink, isBookkeeping, label, meta, target, visibleSteps } from "./steps";
import type { DiskSpace, Op, Plan, Report, Step } from "./types";
import { Code, sentence, Slug } from "./text";

type Phase =
  | { at: "review" }
  | { at: "applying"; started: number }
  | { at: "finished"; report: Report }
  | { at: "error"; message: string };

type PatchState = "inked" | "pending" | "running" | "failed";

interface Props {
  /** The project's root and name. */
  root: string;
  name: string;
  /** The operation the plan is for; applying builds it again from this. */
  op: Op;
  plan: Plan;
  title: string;
  /** The project's own warnings (registry mismatches, a proxy that wouldn't start). */
  warnings?: string[];
  disk: DiskSpace | null;
  /** Bytes the plan's drops free, when the sizes are known. */
  freed?: number;
  /** Move focus to the proof when it opens (it was opened by an action). */
  autoFocus?: boolean;
  /** Called when applying starts, so the app keeps this proof on screen. */
  onStart: () => void;
  /** Close the proof (after it ran, or "Not now"). */
  onClose: () => void;
}

/** A plan drawn as a press proof: colour bar, steps, facts, slug line, sign-off. */
export function Proof({ root, name, op, plan, title, warnings = [], disk, freed = 0, autoFocus, onStart, onClose }: Props) {
  const [phase, setPhase] = useState<Phase>({ at: "review" });
  const [confirmed, setConfirmed] = useState(false);
  const [signedAt, setSignedAt] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (autoFocus) heading.current?.focus();
  }, [autoFocus]);

  // Once the plan has run (or failed), Escape closes the proof like its button
  // does. Never while it's applying.
  useEffect(() => {
    if (phase.at !== "finished" && phase.at !== "error") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase.at, onClose]);

  // The footer is sticky, so a proof taller than the window keeps its sign-off
  // in reach. While it's stuck over the steps, a rule marks where it starts.
  const foot = useRef<HTMLElement>(null);
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const el = foot.current;
    if (!el) return;
    // It sticks 1px past the window's edge, so it's only all in view when it isn't stuck.
    const io = new IntersectionObserver(([e]) => setStuck(e.intersectionRatio < 1), { threshold: 1 });
    io.observe(el);
    return () => io.disconnect();
  }, []);

  const steps = visibleSteps(plan);
  const drops = dropped(plan);
  const destructive = drops.length > 0;
  const records = plan.steps.filter(isBookkeeping).length;
  const queries = [...new Set([...warnings, ...plan.warnings].map(sentence))];
  const added = addedBytes(plan);
  const report = phase.at === "finished" ? phase.report : null;
  const done = new Set(report?.timings.map((t) => t.step) ?? []);
  const timing = new Map(report?.timings.map((t) => [t.step, t.seconds]) ?? []);

  function patch(i: number, index: number): PatchState {
    if (phase.at === "review") return "inked";
    if (phase.at === "applying") {
      if (i < phase.started - 1) return "inked";
      return i === phase.started - 1 ? "running" : "pending";
    }
    if (done.has(index)) return "inked";
    const failedAt = steps.findIndex((s) => !done.has(s.index));
    return phase.at === "error" || i !== failedAt ? "pending" : "failed";
  }

  async function apply() {
    onStart();
    setPhase({ at: "applying", started: 0 });
    let started = 0;
    try {
      const report = await applyOp(root, op, plan, confirmed, (line) => {
        if (line.startsWith("→ ")) {
          started += 1;
          setPhase({ at: "applying", started });
        }
      });
      setPhase({ at: "finished", report });
      setSignedAt(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
    } catch (e) {
      setPhase({ at: "error", message: String(e) });
    }
  }

  const total = report?.timings.reduce((n, t) => n + t.seconds, 0) ?? 0;
  const slug = [
    name,
    plan.kind,
    plural(steps.length, "step"),
    added > 0 ? `+${bytes(added)}` : "no copy",
    destructive ? `drops ${plural(drops.length, "database")}` : "nothing destructive",
    report?.outcome.status === "done" && signedAt ? `applied ${signedAt}` : null,
  ];

  return (
    <section className="sheet" aria-label={`Plan for ${name}`}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />

      {steps.length > 0 && <ol className="bar" aria-hidden="true">
        {steps.map(({ step, index }, i) => (
          <li key={index} className={`patch ink-${ink(step)}`} data-state={patch(i, index)}>
            <span className="patch-ink" />
            <b>{i + 1}</b>
            <span>{label(step)}</span>
          </li>
        ))}
      </ol>}

      <div className="sheet-body">
        <h2 ref={heading} tabIndex={-1}>{title}</h2>

        <ol className="steps">
          {steps.map(({ step, index }, i) => {
            const t = timing.get(index);
            const m = t !== undefined ? seconds(t) : meta(step);
            return (
              <li key={index} data-state={patch(i, index)}>
                <b className={`num ink-${ink(step)}`}>{i + 1}</b>
                <span className="step-text" title={command(step)}>{describe(step)}</span>
                {m && <span className="meta">{m}</span>}
              </li>
            );
          })}
        </ol>
        {records > 0 && (
          <p className="records">Also updates sidetrack's records ({records === 1 ? "1 entry" : `${records} entries`}).</p>
        )}

        {(queries.length > 0 || plan.notes.length > 0) && (
          <div className="queries">
            {queries.map((w) => (
              <p key={w} className="query"><strong>Query</strong> <Code text={w} /></p>
            ))}
            {plan.notes.map((n) => (
              <p key={n} className="note"><Code text={sentence(n)} /></p>
            ))}
          </div>
        )}

        <div className="facts">
          <div>
            <Fact plan={plan} drops={drops} />
          </div>
          <div>
            <h3>Disk on the Docker host</h3>
            {disk ? (
              <p className="big">
                {gb(disk.free_bytes)}
                {added !== freed && <><em>→</em>{gb(disk.free_bytes - added + freed)}</>}
                <small>GB free</small>
              </p>
            ) : (
              <p className="fact-v">Unknown</p>
            )}
          </div>
        </div>
      </div>

      <footer className="sheet-foot" ref={foot} data-stuck={stuck || undefined}>
        <Slug items={slug} />
        <div className={`signoff${destructive ? " spot" : ""}`} aria-live="polite">
          {phase.at === "review" && (
            <>
              <h3>OK to apply</h3>
              {destructive && (
                <label className="confirm">
                  <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
                  <span>{confirmText(plan, drops)} This can't be undone.</span>
                </label>
              )}
              <div className="row">
                <button className="primary apply" disabled={destructive && !confirmed} onClick={apply}>Apply</button>
                <button className="secondary" onClick={onClose}>Not now</button>
              </div>
            </>
          )}
          {phase.at === "applying" && (
            <>
              <h3>Applying</h3>
              <p>Step {Math.max(phase.started, 1)} of {steps.length}. Keep the app open until it finishes.</p>
            </>
          )}
          {report?.outcome.status === "done" && (
            <>
              <h3>Applied</h3>
              <p>{plural(steps.length, "step")} in {seconds(total)}. {doneText(plan, name, drops)}</p>
              <div className="row"><button className="primary" onClick={onClose}>Done</button></div>
            </>
          )}
          {report?.outcome.status === "rolled_back" && (
            <>
              <h3>Not applied</h3>
              <p>“{report.outcome.step}” failed: <Code text={report.outcome.error} /></p>
              <p>
                {report.outcome.undo_errors.length === 0
                  ? "Everything before it was undone, so nothing changed."
                  : `Undoing earlier steps also failed: ${report.outcome.undo_errors.join("; ")}`}
              </p>
              <div className="row"><button className="secondary" onClick={onClose}>Close</button></div>
            </>
          )}
          {report?.outcome.status === "stopped" && (
            <>
              <h3>Stopped</h3>
              <p>“{report.outcome.step}” failed: <Code text={report.outcome.error} /></p>
              <p><Code text={report.outcome.note} /></p>
              <div className="row"><button className="secondary" onClick={onClose}>Close</button></div>
            </>
          )}
          {phase.at === "error" && (
            <>
              <h3>Not applied</h3>
              <p><Code text={phase.message} /></p>
              <div className="row"><button className="primary" onClick={onClose}>Review again</button></div>
            </>
          )}
        </div>
      </footer>
    </section>
  );
}

const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;

/** The databases a plan drops, in order. */
function dropped(plan: Plan): string[] {
  return plan.steps.flatMap((s) => (s.type === "drop_database" ? [s.name] : []));
}

/** The first step of a type, typed. */
function find<T extends Step["type"]>(plan: Plan, type: T): Extract<Step, { type: T }> | undefined {
  return plan.steps.find((s): s is Extract<Step, { type: T }> => s.type === type);
}

/** "a", "a and b", "a, b and c", names in the database face. */
function names(xs: string[]): ReactNode {
  return xs.map((x, i) => (
    <span key={x}>
      {i > 0 && (i === xs.length - 1 ? " and " : ", ")}
      <Db>{x}</Db>
    </span>
  ));
}

/** The left fact: what the plan leaves behind. */
function Fact({ plan, drops }: { plan: Plan; drops: string[] }) {
  const branch = find(plan, "register_branch");
  switch (plan.kind) {
    case "create": {
      const from = find(plan, "clone_database");
      return (
        <>
          <h3>New database</h3>
          <p className="fact-v db nw">{branch?.db_name ?? "?"}</p>
          <p className="fact-sub">
            {from ? <>A copy of <Db>{from.source}</Db></> : "Empty"}, for <Db>{branch?.git_branch}</Db>. The app keeps using its current database.
          </p>
        </>
      );
    }
    case "snapshot": {
      const snap = find(plan, "register_snapshot");
      return (
        <>
          <h3>New snapshot</h3>
          <p className="fact-v db nw">{snap?.db_name ?? "?"}</p>
          <p className="fact-sub">A frozen copy of <Db>{snap?.branch_db}</Db>{snap?.label && <>, labelled “{snap.label}”</>}, kept to reset to.</p>
        </>
      );
    }
    case "reset": {
      const from = find(plan, "clone_database");
      const db = find(plan, "rename_database")?.to;
      return (
        <>
          <h3>Replaced with a copy of</h3>
          <p className="fact-v db nw">{from?.source ?? "?"}</p>
          <p className="fact-sub"><Db>{db}</Db> keeps its name, and the app keeps using it.</p>
        </>
      );
    }
    case "rename": {
      const link = find(plan, "relink_branch");
      const same = link?.new_db_name === link?.db_name;
      return (
        <>
          <h3>{same ? "Keeps its name" : "New name"}</h3>
          <p className="fact-v db nw">{link?.new_db_name ?? "?"}</p>
          <p className="fact-sub">Linked to the git branch <Db>{link?.new_git_branch}</Db>.</p>
        </>
      );
    }
    case "delete":
    case "gc":
    case "snapshot_remove":
      return (
        <>
          <h3>Drops</h3>
          {drops.length === 0 ? (
            <p className="fact-v">Nothing on the server</p>
          ) : (
            <ul className="drops">
              {drops.slice(0, 4).map((d) => <li key={d} className="db">{d}</li>)}
              {drops.length > 4 && <li className="fact-sub">and {drops.length - 4} more</li>}
            </ul>
          )}
        </>
      );
    default:
      if (plan.schema) {
        return (
          <>
            <h3>Schema change</h3>
            <SchemaFacts plan={plan} />
          </>
        );
      }
      return (
        <>
          <h3>The app will use</h3>
          <p className="fact-v db nw">{target(plan) ?? "?"}</p>
          {createsDatabase(plan) && branch && <p className="fact-sub">A new database: <Db>{branch.git_branch}</Db> has none yet.</p>}
        </>
      );
  }
}

/** What a delete plan removes: "feature_x_db and its 2 snapshots". */
function deleted(plan: Plan, drops: string[]): ReactNode {
  const db = find(plan, "forget_branch")?.db_name ?? drops[drops.length - 1];
  const snaps = drops.filter((d) => d !== db).length;
  return <><Db>{db}</Db>{snaps > 0 && <> and its {plural(snaps, "snapshot")}</>}</>;
}

/** The "can't be undone" checkbox, in the plan's own words. */
function confirmText(plan: Plan, drops: string[]): ReactNode {
  if (plan.kind === "delete") return <>Delete {deleted(plan, drops)}.</>;
  if (plan.kind === "reset") {
    const db = find(plan, "rename_database")?.to;
    return <>Replace what's in <Db>{db}</Db> now with a copy of <Db>{find(plan, "clone_database")?.source}</Db>.</>;
  }
  return drops.length <= 3 ? <>Drop {names(drops)}.</> : <>Drop these {drops.length} databases.</>;
}

/** What the sign-off says once the plan applied. */
function doneText(plan: Plan, project: string, drops: string[]): ReactNode {
  switch (plan.kind) {
    case "create": {
      const b = find(plan, "register_branch");
      return <><Db>{b?.db_name}</Db> is ready. Check out <Db>{b?.git_branch}</Db> to use it.</>;
    }
    case "snapshot":
      return <>Saved <Db>{find(plan, "register_snapshot")?.db_name}</Db>.</>;
    case "reset": {
      const db = find(plan, "rename_database")?.to;
      return <><Db>{db}</Db> is a fresh copy of <Db>{find(plan, "clone_database")?.source}</Db>.</>;
    }
    case "rename": {
      const link = find(plan, "relink_branch");
      return <><Db>{link?.new_db_name}</Db> now belongs to <Db>{link?.new_git_branch}</Db>.</>;
    }
    case "delete":
      return <>Deleted {deleted(plan, drops)}.</>;
    case "gc":
    case "snapshot_remove":
      if (drops.length === 0) return <>Updated sidetrack's records.</>;
      return drops.length <= 3 ? <>Dropped {names(drops)}.</> : <>Dropped {drops.length} databases.</>;
    default:
      return <>{project} now uses <Db>{target(plan)}</Db>.</>;
  }
}

function SchemaFacts({ plan }: { plan: Plan }) {
  const s = plan.schema!;
  const short = (t: string) => t.replace(/^public\./, "");
  const lines: string[] = [
    ...s.added_tables.map((t) => `+ table ${short(t)}`),
    ...s.removed_tables.map((t) => `− table ${short(t)}`),
    ...s.changed_tables.flatMap((c) => [
      ...c.added_columns.map((col) => `+ column ${short(c.table)}.${col.split(" ")[0]}`),
      ...c.removed_columns.map((col) => `− column ${short(c.table)}.${col.split(" ")[0]}`),
    ]),
  ];
  if (lines.length === 0) return <p className="fact-v">Same tables and columns as <span className="db">{s.from_db}</span></p>;
  return (
    <ul className="schema">
      {lines.slice(0, 4).map((l) => <li key={l}>{l}</li>)}
      {lines.length > 4 && <li className="fact-sub">and {lines.length - 4} more</li>}
    </ul>
  );
}
