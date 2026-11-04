import type { ReactNode } from "react";
import { Code, sentence, Slug } from "./text";
import type { ProjectView } from "./types";

function Sheet({ label, children }: { label: string; children: ReactNode }) {
  return (
    <section className="sheet plain" aria-label={label}>
      <i className="crop tl" /><i className="crop tr" /><i className="crop bl" /><i className="crop br" />
      <div className="sheet-body">{children}</div>
    </section>
  );
}

/** A project whose database matches its branch: nothing to sign off. */
export function InSyncSheet({ p }: { p: ProjectView }) {
  const m = p.migrations;
  return (
    <Sheet label={`${p.name} is in sync`}>
      <h2>{p.name} is in sync</h2>
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

/** The project's state couldn't be read: say what's wrong and how to fix it. */
export function ProblemSheet({ p }: { p: ProjectView }) {
  return (
    <Sheet label={`Can't read ${p.name}`}>
      <h2>Can't read {p.name}</h2>
      <p className="problem-text"><Code text={sentence(p.problem ?? "")} /></p>
      <p className="note">
        {p.git_branch ? <>Checked out: <span className="db">{p.git_branch}</span>. </> : null}
        {p.active_db ? <>The app points at <span className="db">{p.active_db}</span>. </> : null}
        Fix it, then refresh.
      </p>
    </Sheet>
  );
}

export function EmptySheet() {
  return (
    <Sheet label="No projects yet">
      <h2>No projects yet</h2>
      <p className="problem-text">
        <Code text="Run `sidetrack init` in a repository whose docker-compose has a Postgres service. It shows up here once it's set up." />
      </p>
    </Sheet>
  );
}

export function LoadErrorSheet({ message }: { message: string }) {
  return (
    <Sheet label="Can't read the registry">
      <h2>Can't read your projects</h2>
      <p className="problem-text"><Code text={sentence(message)} /></p>
    </Sheet>
  );
}

function Queries({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;
  return (
    <div className="queries">
      {warnings.map((w) => (
        <p key={w} className="query"><strong>Query</strong> <Code text={sentence(w)} /></p>
      ))}
    </div>
  );
}
