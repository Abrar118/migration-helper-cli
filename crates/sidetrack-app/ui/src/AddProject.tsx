import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { addProject, previewProject, startDatabase } from "./api";
import { Problem, Queries, Sheet } from "./Sheets";
import { Code, sentence, Slug } from "./text";
import type { AddPreview } from "./types";

type Phase =
  | { at: "reading" }
  | { at: "unreadable"; message: string }
  | { at: "review"; p: AddPreview; startError?: string }
  | { at: "starting"; p: AddPreview }
  | { at: "adding"; p: AddPreview }
  | { at: "added"; p: AddPreview; said: string[] }
  | { at: "failed"; p: AddPreview; message: string };

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;

/**
 * Add the repo at (or above) `dir`: what `sidetrack init` found, drawn as a
 * proof with an "OK to add" sign-off. Nothing is written until Add, and Add is
 * refused if the setup changed since it was shown.
 */
export function AddProject({ dir, onPickAgain, onCancel, onDone }: {
  dir: string;
  onPickAgain: () => void;
  onCancel: () => void;
  /** Added: the project's root, to select it. */
  onDone: (root: string) => void;
}) {
  const [phase, setPhase] = useState<Phase>({ at: "reading" });
  const [hook, setHook] = useState(true);
  const heading = useRef<HTMLHeadingElement>(null);
  const folder = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

  const read = useCallback(async () => {
    setPhase({ at: "reading" });
    try {
      const p = await previewProject(dir);
      setHook(!p.hook_installed);
      setPhase({ at: "review", p });
    } catch (e) {
      setPhase({ at: "unreadable", message: message(e) });
    }
  }, [dir]);

  useEffect(() => {
    read();
  }, [read]);
  useEffect(() => {
    if (phase.at === "review" || phase.at === "unreadable") heading.current?.focus();
  }, [phase.at]);

  const busy = phase.at === "reading" || phase.at === "starting" || phase.at === "adding";
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || busy) return;
      if (phase.at === "added") onDone(phase.p.root);
      else onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, phase, onCancel, onDone]);

  if (phase.at === "reading") {
    return (
      <Sheet label={`Reading ${folder}`} title={`Add ${folder}`}>
        <p className="loading">Reading its compose setup…</p>
      </Sheet>
    );
  }
  if (phase.at === "unreadable") {
    return (
      <Sheet label={`Can't add ${folder}`} title={<span ref={heading} tabIndex={-1}>Can't add {folder}</span>}>
        <Problem text={phase.message} />
        <p className="note">sidetrack needs a git repository whose docker-compose has a Postgres service.</p>
        <div className="row standalone-row">
          <button className="secondary" onClick={onPickAgain}>Choose another folder</button>
          <button className="text-button" onClick={onCancel}>Cancel</button>
        </div>
      </Sheet>
    );
  }

  const p = phase.p;
  const d = p.detected;
  const c = p.config;
  const active = d?.active_db ?? null;
  const start = async () => {
    setPhase({ at: "starting", p });
    try {
      await startDatabase(p.root);
      const again = await previewProject(p.root);
      setPhase({ at: "review", p: again, startError: again.database_running ? undefined : "it didn't come up; check `docker compose logs`" });
    } catch (e) {
      setPhase({ at: "review", p, startError: message(e) });
    }
  };
  const add = async () => {
    setPhase({ at: "adding", p });
    try {
      setPhase({ at: "added", p, said: await addProject(p.root, p.toml, hook) });
    } catch (e) {
      setPhase({ at: "failed", p, message: message(e) });
    }
  };

  const queries = [
    ...(d?.notes ?? []),
    ...(p.registered ? ["it's already added; adding it again only links what's new"] : []),
    ...(p.database_running ? [] : [`the database (${c.database.service}) isn't running, so sidetrack can't check what's in it yet`]),
  ];
  const proxy = c.routing.mode === "proxy";

  return (
    <section className="sheet add-sheet" aria-label={`Add ${p.name}`}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />
      <div className="sheet-body">
        <h2 ref={heading} tabIndex={-1}>Add {p.name}</h2>
        <p className="add-path db">{p.root}</p>

        <Queries warnings={queries} />
        {!p.database_running && (
          <div className="row add-start">
            <button className="secondary" onClick={start} disabled={phase.at !== "review"}>
              {phase.at === "starting" ? "Starting…" : "Start the database"}
            </button>
            {phase.at === "review" && phase.startError && <Problem text={phase.startError} />}
          </div>
        )}

        <div className="facts">
          <div>
            <h3>Database</h3>
            <p className="fact-v db nw">{c.database.service}</p>
            <p className="fact-sub">
              {d ? `${d.db_image}, ` : ""}user <Db>{c.database.user}</Db>
              {active && <>. The app uses <Db>{active}</Db> now.</>}
            </p>
          </div>
          <div>
            <h3>Switching branches</h3>
            {proxy ? (
              <>
                <p className="fact-v">Through the proxy</p>
                <p className="fact-sub">The app connects to <Db>sidetrack proxy</Db>, which picks the database.</p>
              </>
            ) : (
              <>
                <p className="fact-v db nw">{c.routing.variable}</p>
                <p className="fact-sub">
                  Rewritten in <Db>{c.routing.file}</Db>
                  {c.routing.restart.length > 0 && (
                    <>, then {c.routing.restart.map((s, i) => <span key={s}>{i > 0 && ", "}<Db>{s}</Db></span>)} {c.routing.restart.length === 1 ? "is" : "are"} recreated</>
                  )}.
                </p>
              </>
            )}
          </div>
          <div>
            <h3>Migrations</h3>
            <p className="fact-v">{c.migrations.tool}</p>
            <p className="fact-sub">
              In <Db>{c.migrations.dirs.join(", ")}</Db>, run with <Code text={`\`${c.migrations.command}\``} />.
            </p>
          </div>
          <div>
            <h3>Checked out</h3>
            <p className="fact-v db nw">{p.branch ?? "a detached HEAD"}</p>
            <p className="fact-sub">
              {c.git.main_branch ? <>Features merge into <Db>{c.git.main_branch}</Db>.</> : "No main branch found; Clean up won't spot merged branches."}
            </p>
          </div>
        </div>

        <details className="toml">
          <summary>{d ? "The sidetrack.toml it writes" : "Its sidetrack.toml (already in the repo)"}</summary>
          <pre>{p.toml}</pre>
        </details>
      </div>

      <footer className="sheet-foot">
        <Slug
          items={[
            p.name,
            proxy ? "proxy" : "env file",
            c.migrations.tool,
            d ? "new sidetrack.toml" : "existing sidetrack.toml",
            p.database_running ? `${c.database.service} running` : `${c.database.service} stopped`,
          ]}
        />
        <div className="signoff" aria-live="polite">
          {(phase.at === "review" || phase.at === "starting") && (
            <>
              <h3>OK to add</h3>
              <p>
                {d ? <>Writes <Db>sidetrack.toml</Db> to the repo and links </> : <>Links </>}
                <Db>{p.branch ?? "the checked-out branch"}</Db> to {active ? <Db>{active}</Db> : "the database the app uses now"}.
              </p>
              {!p.hook_installed && (
                <label className="confirm key">
                  <input type="checkbox" checked={hook} onChange={(e) => setHook(e.target.checked)} />
                  <span>Install the git hook, so checking out a branch offers to switch its database.</span>
                </label>
              )}
              <div className="row">
                <button className="primary" onClick={add} disabled={!p.database_running || phase.at !== "review"}>Add</button>
                <button className="secondary" onClick={onCancel}>Cancel</button>
              </div>
            </>
          )}
          {phase.at === "adding" && (
            <>
              <h3>Adding</h3>
              <p>Linking <Db>{p.branch}</Db> and reading its database.</p>
            </>
          )}
          {phase.at === "added" && (
            <>
              <h3>Added</h3>
              {phase.said.map((s) => <p key={s}><Code text={sentence(s.replace(/^! /, ""))} /></p>)}
              <div className="row"><button className="primary" autoFocus onClick={() => onDone(p.root)}>Done</button></div>
            </>
          )}
          {phase.at === "failed" && (
            <>
              <h3>Not added</h3>
              <p><Code text={sentence(phase.message)} /></p>
              <div className="row">
                <button className="secondary" onClick={read}>Review again</button>
                <button className="text-button" onClick={onCancel}>Cancel</button>
              </div>
            </>
          )}
        </div>
      </footer>
    </section>
  );
}
