import type { ReactNode } from "react";
import { Code, sentence, Slug } from "./text";
import type { ProjectView } from "./types";

/**
 * A sheet with nothing to sign off. `inRegister` prints the registration
 * target beside the title: sidetrack checked and everything lines up.
 */
export function Sheet({ label, title, inRegister = false, children }: {
  label: string;
  title: ReactNode;
  inRegister?: boolean;
  children: ReactNode;
}) {
  return (
    <section className="sheet plain" aria-label={label}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />
      <div className="sheet-body">
        <div className="sheet-head">
          <h2>{title}</h2>
          {inRegister && <Register />}
        </div>
        {children}
      </div>
    </section>
  );
}

/** A registration target, in key like the crop marks. Hidden from screen readers: the sheet's title says it. */
function Register() {
  return (
    <svg className="register" viewBox="0 0 45 45" aria-hidden="true">
      <circle cx="22.5" cy="22.5" r="15" />
      <line x1="0" y1="22.5" x2="45" y2="22.5" />
      <line x1="22.5" y1="0" x2="22.5" y2="45" />
      <circle className="dot" cx="22.5" cy="22.5" r="7" />
      <line className="knock" x1="15.5" y1="22.5" x2="29.5" y2="22.5" />
      <line className="knock" x1="22.5" y1="15.5" x2="22.5" y2="29.5" />
    </svg>
  );
}

/** A project whose database matches its branch: nothing to sign off. */
export function InSyncSheet({ p }: { p: ProjectView }) {
  const m = p.migrations;
  return (
    <Sheet label={`${p.name} is in sync`} title={`${p.name} is in sync`} inRegister>
      <p className="route">
        <span className="db">{p.git_branch}</span>
        <em>→</em>
        <span className="db">{p.active_db}</span>
      </p>
      <Slug
        className="standalone"
        items={[
          `${p.branches} db ${p.branches === 1 ? "branch" : "branches"}`,
          `${p.snapshots} ${p.snapshots === 1 ? "snapshot" : "snapshots"}`,
          m && `${m.applied} migrations ${m.source}`,
          m && (m.pending.length > 0 || m.unknown.length > 0) && `${m.pending.length} pending, ${m.unknown.length} unknown`,
          p.running_services.length > 0 ? `${p.running_services.join(", ")} running` : "nothing running",
        ]}
      />
      {p.mode === "proxy" && (
        <p className="note">
          The app connects through the proxy on {p.listen}
          {p.proxy_running === false && ", which isn't running. Start it with `sidetrack proxy --daemon`."}
          {p.proxy_running !== false && "."}
        </p>
      )}
      {p.elsewhere.map(([db, wt]) => (
        <p key={db} className="note">Another worktree, <span className="db">{wt}</span>, uses <span className="db">{db}</span>.</p>
      ))}
      <Queries warnings={p.warnings} />
    </Sheet>
  );
}

/** What couldn't be read, as a proofreader's query: it needs an answer before anything else. */
export function Problem({ text }: { text: string }) {
  return <p className="query problem-text"><strong>Query</strong> <Code text={sentence(text)} /></p>;
}

/** The project's state couldn't be read: say what's wrong and how to fix it. */
export function ProblemSheet({ p }: { p: ProjectView }) {
  return (
    <Sheet label={`Can't read ${p.name}`} title={`Can't read ${p.name}`}>
      <Problem text={p.problem ?? ""} />
      <p className="note">
        {p.git_branch ? <>Checked out: <span className="db">{p.git_branch}</span>. </> : null}
        {p.active_db ? <>The app points at <span className="db">{p.active_db}</span>. </> : null}
        Fix it, then refresh.
      </p>
    </Sheet>
  );
}

export function EmptySheet({ registry }: { registry: string }) {
  return (
    <Sheet label="No projects yet" title="No projects yet">
      <p className="problem-text">
        <Code text="Run `sidetrack init` in a repository whose docker-compose has a Postgres service. It shows up here once it's set up." />
      </p>
      <Slug className="standalone" items={["0 projects", registry]} />
    </Sheet>
  );
}

export function LoadErrorSheet({ message }: { message: string }) {
  return (
    <Sheet label="Can't read the registry" title="Can't read your projects">
      <Problem text={message} />
    </Sheet>
  );
}

export function Queries({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;
  return (
    <div className="queries">
      {warnings.map((w) => (
        <p key={w} className="query"><strong>Query</strong> <Code text={sentence(w)} /></p>
      ))}
    </div>
  );
}
