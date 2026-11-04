import { useState } from "react";
import { applySwitch } from "./api";
import { bytes, gb, plural, seconds } from "./format";
import { addedBytes, command, createsDatabase, describe, ink, isBookkeeping, isDestructive, label, meta, target, visibleSteps } from "./steps";
import type { DiskSpace, Plan, ProjectView, Report } from "./types";
import { Code, Slug } from "./text";

type Phase =
  | { at: "review" }
  | { at: "applying"; started: number }
  | { at: "finished"; report: Report }
  | { at: "error"; message: string };

type PatchState = "inked" | "pending" | "running" | "failed";

interface Props {
  project: ProjectView;
  plan: Plan;
  disk: DiskSpace | null;
  /** Called when applying starts, so the app keeps this proof on screen. */
  onStart: () => void;
  /** Close the proof (after it ran, or "Not now"). */
  onClose: () => void;
}

/** A plan drawn as a press proof: colour bar, steps, facts, slug line, sign-off. */
export function Proof({ project, plan, disk, onStart, onClose }: Props) {
  const [phase, setPhase] = useState<Phase>({ at: "review" });
  const [confirmed, setConfirmed] = useState(false);
  const [signedAt, setSignedAt] = useState<string | null>(null);

  const steps = visibleSteps(plan);
  const drops = plan.steps.filter(isDestructive).map((s) => (s.type === "drop_database" ? s.name : ""));
  const destructive = drops.length > 0;
  const records = plan.steps.filter(isBookkeeping).length;
  const added = addedBytes(plan);
  const to = target(plan);
  const branch = project.git_branch ?? "this branch";
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
      const report = await applySwitch(project.root, plan, confirmed, (line) => {
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
    project.name,
    plan.kind,
    plural(steps.length, "step"),
    added > 0 ? `+${bytes(added)}` : "no copy",
    destructive ? `drops ${plural(drops.length, "database")}` : "nothing destructive",
    report?.outcome.status === "done" && signedAt ? `applied ${signedAt}` : null,
  ];

  return (
    <section className="sheet" aria-label={`Plan for ${project.name}`}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />

      <ol className="bar" aria-hidden="true">
        {steps.map(({ step, index }, i) => (
          <li key={index} className={`patch ink-${ink(step)}`} data-state={patch(i, index)}>
            <span className="patch-ink" />
            <b>{i + 1}</b>
            <span>{label(step)}</span>
          </li>
        ))}
      </ol>

      <div className="sheet-body">
        <h2>Switch {project.name} to {branch}</h2>

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

        {(plan.warnings.length > 0 || plan.notes.length > 0) && (
          <div className="queries">
            {plan.warnings.map((w) => (
              <p key={w} className="query"><strong>Query</strong> <Code text={w} /></p>
            ))}
            {plan.notes.map((n) => (
              <p key={n} className="note"><Code text={n} /></p>
            ))}
          </div>
        )}

        <div className="facts">
          <div>
            {plan.schema ? (
              <>
                <h3>Schema change</h3>
                <SchemaFacts plan={plan} />
              </>
            ) : (
              <>
                <h3>The app will use</h3>
                <p className="fact-v db nw">{to ?? "?"}</p>
                {createsDatabase(plan) && <p className="fact-sub">A new database: {branch} has none yet.</p>}
              </>
            )}
          </div>
          <div>
            <h3>Disk on the Docker host</h3>
            {disk ? (
              <p className="big">
                {gb(disk.free_bytes)}
                {added > 0 && <><em>→</em>{gb(disk.free_bytes - added)}</>}
                <small>GB free</small>
              </p>
            ) : (
              <p className="fact-v">Unknown</p>
            )}
          </div>
        </div>
      </div>

      <footer className="sheet-foot">
        <Slug items={slug} />
        <div className={`signoff${destructive ? " spot" : ""}`} aria-live="polite">
          {phase.at === "review" && (
            <>
              <h3>OK to apply</h3>
              {destructive && (
                <label className="confirm">
                  <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
                  <span>Drop <span className="db">{drops.join(", ")}</span>. This can't be undone.</span>
                </label>
              )}
              <div className="row">
                <button className="primary" disabled={destructive && !confirmed} onClick={apply}>Apply</button>
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
              <p>{plural(steps.length, "step")} in {seconds(total)}. {project.name} now uses <span className="db">{to}</span>.</p>
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
