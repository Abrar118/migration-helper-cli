import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { addProject, pickComposeFiles, previewProject, startDatabase } from "./api";
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
  const [composeFiles, setComposeFiles] = useState<string[]>([]);
  const [useProxy, setUseProxy] = useState(false);
  const [proxyReady, setProxyReady] = useState(false);
  const request = useRef(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const folder = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

  const read = useCallback(async () => {
    const id = ++request.current;
    setPhase({ at: "reading" });
    setProxyReady(false);
    try {
      const p = await previewProject(dir, composeFiles, useProxy);
      if (id !== request.current) return;
      setHook(!p.hook_installed);
      setPhase({ at: "review", p });
    } catch (e) {
      if (id !== request.current) return;
      setPhase({ at: "unreadable", message: message(e) });
    }
  }, [dir, composeFiles, useProxy]);

  useEffect(() => {
    read();
    return () => { request.current++; };
  }, [read]);
  useEffect(() => {
    if (phase.at === "review" || phase.at === "unreadable") heading.current?.focus();
  }, [phase.at]);

  const busy = phase.at === "reading" || phase.at === "starting" || phase.at === "adding";
  const existing = "p" in phase && phase.p.detected === null;
  const files = "p" in phase ? phase.p.config.compose?.files ?? composeFiles : composeFiles;
  const choices = (
    <div className="add-setup">
      <p className="note">Compose files: <Db>{files.length ? files.join(" → ") : "Docker Compose defaults"}</Db></p>
      {!existing && composeFiles.length > 0 && <ol aria-label="Compose files in merge order">
        {composeFiles.map((file, index) => <li key={`${index}:${file}`}>
          <Db>{file}</Db>{" "}
          <button className="text-button" disabled={busy || index === 0} aria-label={`Move ${file} earlier`} onClick={() => setComposeFiles(files => {
            const next = [...files];
            [next[index - 1], next[index]] = [next[index], next[index - 1]];
            return next;
          })}>Move earlier</button>{" "}
          <button className="text-button" disabled={busy} aria-label={`Remove ${file}`} onClick={() => setComposeFiles(files => files.filter((_, i) => i !== index))}>Remove</button>
        </li>)}
      </ol>}
      {!existing && <div className="row">
        <button className="secondary" disabled={busy} onClick={async () => {
          try { const picked = await pickComposeFiles(dir); if (picked?.length) setComposeFiles(picked); }
          catch (e) { setPhase({ at: "unreadable", message: message(e) }); }
        }}>Choose Compose file</button>
        {composeFiles.length > 0 && <button className="secondary" disabled={busy} onClick={async () => {
          try { const picked = await pickComposeFiles(dir, true); if (picked?.length) setComposeFiles(files => [...files, ...picked]); }
          catch (e) { setPhase({ at: "unreadable", message: message(e) }); }
        }}>Add override file</button>}
        {composeFiles.length > 0 && <button className="text-button" disabled={busy} onClick={() => setComposeFiles([])}>Use default files</button>}
      </div>}
      {!existing && <label className="confirm key">
        <input type="checkbox" checked={useProxy} disabled={busy} onChange={(e) => setUseProxy(e.target.checked)} />
        <span>Use proxy routing. Your app connects through Sidetrack instead of rewriting an env file.</span>
      </label>}
      {existing && <p className="note">Using the settings in this project's sidetrack.toml.</p>}
    </div>
  );
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
        {choices}
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
      await startDatabase(p.root, p.toml);
      const again = await previewProject(p.root, composeFiles, useProxy);
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
    ...p.warnings,
    ...(p.registered ? ["it's already added; adding it again only links what's new"] : []),
    ...(p.database_running ? [] : [`the database (${c.database.service}) isn't running, so sidetrack can't check what's in it yet`]),
    ...(c.routing.mode === "proxy" && p.database_running && !p.proxy_upstream ? ["the database must publish port 5432 for the proxy; publish a local port in Compose or set routing.upstream in sidetrack.toml"] : []),
  ];
  const proxy = c.routing.mode === "proxy";
  const listen = c.routing.listen ?? "127.0.0.1:54320";
  const proxyPort = listen.slice(listen.lastIndexOf(":") + 1);

  return (
    <section className="sheet add-sheet" aria-label={`Add ${p.name}`}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />
      <div className="sheet-body">
        <h2 ref={heading} tabIndex={-1}>Add {p.name}</h2>
        <p className="add-path db">{p.root}</p>
        {phase.at !== "added" && choices}

        <Queries warnings={queries} />
        {proxy && d && phase.at !== "added" && <div className="add-setup">
          <h3>Connect your app to the proxy</h3>
          <p>After adding, point {d.inline.length ? d.inline.map((item) => `${item.variable} in ${item.service}`).join(", ") : "your app's database connection"} at <Db>host.docker.internal:{proxyPort}</Db> from containers, or <Db>127.0.0.1:{proxyPort}</Db> from this machine. Keep the database name <Db>{c.routing.database}</Db>, credentials, JDBC prefix and URL options.</p>
          <p className="note">Add the project first to save these settings and start its proxy, then update the app connection and recreate its containers. The proxy listens on <Db>{listen}</Db>. On Linux, containers also need <Db>host.docker.internal:host-gateway</Db> in extra_hosts.</p>
          <label className="confirm key"><input type="checkbox" checked={proxyReady} disabled={busy} onChange={(e) => setProxyReady(e.target.checked)} /><span>I will update the app connection after adding.</span></label>
          <button className="secondary" disabled={busy} onClick={read}>Review setup again</button>
        </div>}
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
                {proxy && <> Starts the proxy if automatic startup is enabled. Update the app connection and recreate its containers afterward.</>}
              </p>
              {!p.hook_installed && (
                <label className="confirm key">
                  <input type="checkbox" checked={hook} onChange={(e) => setHook(e.target.checked)} />
                  <span>Install the git hook, so checking out a branch offers to switch its database.</span>
                </label>
              )}
              <div className="row">
                <button className="primary" onClick={add} disabled={!p.database_running || phase.at !== "review" || (proxy && (!p.proxy_upstream || (d !== null && !proxyReady)))}>Add</button>
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
