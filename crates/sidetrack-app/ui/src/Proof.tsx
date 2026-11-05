import { useEffect, useRef, useState, type ReactNode } from "react";
import { applyOp } from "./api";
import { bytes, gb, plural, seconds } from "./format";
import { addedBytes, command, createsDatabase, describe, ink, isBookkeeping, label, meta, pendingOnly, target, visibleSteps } from "./steps";
import type { DiskSpace, Op, Plan, Report, Step } from "./types";
import { Code, sentence, Slug } from "./text";
import { usePlain } from "./look";

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
  const plain = usePlain();
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
    plan.kind.replace(/_/g, " "),
    plural(steps.length, "step"),
    added > 0 ? `+${bytes(added)}` : "no copy",
    destructive ? `drops ${plural(drops.length, "database")}` : "nothing destructive",
    report?.outcome.status === "done" && signedAt ? `applied ${signedAt}` : null,
  ];

  return (
    <section className="sheet" aria-label={`Plan for ${name}`}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />

      {!plain && steps.length > 0 && <ol className="bar" aria-hidden="true">
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
        {plain && <BeforeAfter plan={plan} drops={drops} />}

        {plain && <h3 className="steps-head">What happens when you apply</h3>}
        <ol className="steps">
          {steps.map(({ step, index }, i) => {
            const t = timing.get(index);
            const m = t !== undefined ? seconds(t) : meta(step);
            return (
              <li key={index} data-state={patch(i, index)}>
                <b className={`num ink-${ink(step)}`}>{i + 1}</b>
                <span className="step-text" title={command(step)}>{describe(step)}</span>
                {m && <span className="meta">{m}</span>}
                {plain && <span className={`step-kind ink-${ink(step)}`}>{label(step)}</span>}
              </li>
            );
          })}
        </ol>
        {records > 0 && (
          <p className="records">Also updates sidetrack's records ({records === 1 ? "1 entry" : `${records} entries`}).</p>
        )}

        {plain && (queries.length > 0 || plan.notes.length > 0) && (
          <div className="callout">
            <h3>Worth knowing</h3>
            {queries.map((w) => <p key={w}><Code text={w} /></p>)}
            {plan.notes.map((n) => <p key={n} className="note"><Code text={sentence(n)} /></p>)}
          </div>
        )}
        {!plain && (queries.length > 0 || plan.notes.length > 0) && (
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
            <h3>{plain ? "Free disk space on the Docker host" : "Disk on the Docker host"}</h3>
            {disk ? (
              <p className="big">
                {gb(disk.free_bytes)}
                {added !== freed && <><em>→</em>{gb(disk.free_bytes - added + freed)}</>}
                <small>GB free</small>
              </p>
            ) : (
              <p className="fact-v">Unknown</p>
            )}
            {plain && disk && (
              <p className="fact-sub">
                {added === freed ? "Unchanged: nothing is copied or deleted." : added > freed ? `This plan copies ${bytes(added - freed)} of data.` : `Deleting frees ${bytes(freed - added)}.`}
              </p>
            )}
          </div>
        </div>
      </div>

      <footer className="sheet-foot" ref={foot} data-stuck={stuck || undefined}>
        {plain ? (
          <p className="plan-sum">
            {plural(steps.length, "step")} · {added > 0 ? `copies ${bytes(added)}` : "no copy"} · {destructive ? `deletes ${plural(drops.length, "database")}` : "nothing is deleted"}
            {report?.outcome.status === "done" && signedAt && ` · applied at ${signedAt}`}
          </p>
        ) : (
          <Slug items={slug} />
        )}
        <div className={`signoff${destructive ? " spot" : ""}`} aria-live="polite">
          {phase.at === "review" && (
            <>
              <h3>{plain ? (destructive ? "Ready to apply, and it deletes data" : "Ready to apply") : "OK to apply"}</h3>
              {plain && !destructive && <p className="signoff-note">Nothing runs until you click Apply.</p>}
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
  const plain = usePlain();
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
            <h3>{plain ? "Tables and columns the app will see" : "Schema change"}</h3>
            <SchemaFacts plan={plan} />
          </>
        );
      }
      const pending = pendingOnly(plan);
      if (pending) {
        return (
          <>
            <h3>Migrations to run</h3>
            <ul className="drops">
              {pending.slice(0, 4).map((m) => <li key={m} className="db">{m}</li>)}
              {pending.length > 4 && <li className="fact-sub">and {pending.length - 4} more</li>}
            </ul>
            <p className="fact-sub">On the database the app already uses.</p>
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
    default: {
      const pending = pendingOnly(plan);
      if (pending) return <>Ran {plural(pending.length, "migration")} on the database {project} uses.</>;
      return <>{project} now uses <Db>{target(plan)}</Db>.</>;
    }
  }
}

/**
 * The Canvas look's opening: what's true now and what will be after Apply,
 * with one sentence on why. Only for plans that move or copy a database.
 */
function BeforeAfter({ plan, drops }: { plan: Plan; drops: string[] }) {
  const route = plan.steps.find((s): s is Extract<Step, { type: "rewrite_env" | "set_proxy_target" }> => s.type === "rewrite_env" || s.type === "set_proxy_target");
  const clone = find(plan, "clone_database");
  const branch = find(plan, "register_branch");
  let now: [string, ReactNode], after: [string, ReactNode], why: ReactNode;
  if (route) {
    now = ["The app uses now", <Db>{route.from_db}</Db>];
    after = ["After you apply", <Db>{route.to_db}</Db>];
    why = clone
      ? <><Db>{branch?.git_branch}</Db> has no database yet, so a copy of <Db>{clone.source}</Db> is made for it first. Then the app is pointed at it.</>
      : <>The checked-out git branch already has its own database, so the app is pointed at it. <Db>{route.from_db}</Db> is kept as it is; switching back later is just as quick.</>;
  } else if (plan.kind === "create" && branch) {
    now = ["Copied from", clone ? <Db>{clone.source}</Db> : "Nothing (empty)"];
    after = ["New database", <Db>{branch.db_name}</Db>];
    why = <>A database for <Db>{branch.git_branch}</Db>, ready for when you check it out. The app keeps using its current database.</>;
  } else if (plan.kind === "reset" && clone) {
    const db = find(plan, "rename_database")?.to;
    now = ["Database", <Db>{db}</Db>];
    after = ["Replaced with a fresh copy of", <Db>{clone.source}</Db>];
    why = <>Everything written to <Db>{db}</Db> since it was made is thrown away. It keeps its name, so the app doesn't need to change.</>;
  } else if (drops.length > 0) {
    now = ["Deletes", drops.length === 1 ? <Db>{drops[0]}</Db> : `${drops.length} databases`];
    after = ["Afterwards", "Gone from the server"];
    why = <>Deleted databases can't be brought back. Check the list below before you apply.</>;
  } else {
    return null;
  }
  return (
    <div className="change">
      <div className="change-row">
        <div className="change-box"><span className="change-k">{now[0]}</span><span className="change-v">{now[1]}</span></div>
        <span className="change-arrow" aria-hidden="true">→</span>
        <div className="change-box after"><span className="change-k">{after[0]}</span><span className="change-v">{after[1]}</span></div>
      </div>
      <p className="change-why">{why}</p>
    </div>
  );
}

function SchemaFacts({ plan }: { plan: Plan }) {
  const plain = usePlain();
  if (plain && plan.schema) {
    const s = plan.schema;
    const short = (t: string) => t.replace(/^public\./, "");
    const rows: [boolean, string][] = [
      ...s.added_tables.map((t): [boolean, string] => [true, `table ${short(t)}`]),
      ...s.removed_tables.map((t): [boolean, string] => [false, `table ${short(t)}`]),
      ...s.changed_tables.flatMap((c) => [
        ...c.added_columns.map((col): [boolean, string] => [true, `column ${short(c.table)}.${col.split(" ")[0]}`]),
        ...c.removed_columns.map((col): [boolean, string] => [false, `column ${short(c.table)}.${col.split(" ")[0]}`]),
      ]),
    ];
    return (
      <>
        <p className="fact-sub">
          <Db>{s.to_db}</Db> compared with <Db>{s.from_db}</Db>, the one the app uses now.
        </p>
        {rows.length === 0 ? (
          <p className="fact-v">Same tables and columns</p>
        ) : (
          <ul className="schema-plain">
            {rows.slice(0, 6).map(([add, l]) => (
              <li key={l}><span className={`badge ${add ? "ok" : "bad"}`}>{add ? "added" : "not there"}</span> <span className="db">{l}</span></li>
            ))}
            {rows.length > 6 && <li className="fact-sub">and {rows.length - 6} more</li>}
          </ul>
        )}
      </>
    );
  }
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
