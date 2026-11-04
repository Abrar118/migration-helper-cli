import { Fragment, useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { loadDetail } from "./api";
import { ago, bytes, plural } from "./format";
import { Queries, Sheet } from "./Sheets";
import { Code, sentence, Slug } from "./text";
import type { DbBranchView, GitBranchView, ProjectDetail, ServiceView } from "./types";

/** What's selected. A git branch and its DB branch are selected together. */
type Selection = { git: string } | { db: string };

interface Props {
  root: string;
  /** The wordmark and main navigation, shared with All projects. */
  top: ReactNode;
  /** The project's switch plan is waiting on All projects. */
  planWaiting: boolean;
  onBack: () => void;
}

/**
 * One project as two linked panes: its git branches in the column, its DB
 * branches as a parent → child tree on the sheet. Read-only.
 */
export function ProjectPage({ root, top, planWaiting, onBack }: Props) {
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [sel, setSel] = useState<Selection | null>(null);
  const busy = useRef(false);

  const refresh = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setLoading(true);
    try {
      const d = await loadDetail(root);
      setDetail(d);
      setError(null);
      // Keep the selection while it still exists; else the checked-out branch.
      setSel((cur) => {
        if (cur && "git" in cur && d.git_branches.some((b) => b.name === cur.git)) return cur;
        if (cur && "db" in cur && d.db_branches.some((b) => b.db_name === cur.db)) return cur;
        const first = d.git_branches[0];
        return first ? { git: first.name } : d.db_branches[0] ? { db: d.db_branches[0].db_name } : null;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }, [root]);

  // Like All projects: read again whenever the window comes back to the front.
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") onBack();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack]);

  const name = detail?.name ?? root.split(/[\\/]/).pop() ?? root;
  const gitSel = sel && "git" in sel ? detail?.git_branches.find((b) => b.name === sel.git) ?? null : null;
  const dbSel =
    sel && "db" in sel
      ? detail?.db_branches.find((b) => b.db_name === sel.db) ?? null
      : gitSel?.db
        ? detail?.db_branches.find((b) => b.db_name === gitSel.db) ?? null
        : null;
  // A DB branch picked in the tree selects its git branch too, when it still exists.
  const gitShown = gitSel ?? (dbSel?.git_branch_exists ? detail?.git_branches.find((b) => b.name === dbSel.git_branch) ?? null : null);

  return (
    <>
      <aside className="col">
        {top}
        <div className="back">
          <button className="text-button" onClick={onBack}>Back to all projects</button>
        </div>
        <h1 className="project-title">{name}</h1>
        {detail === null && !error ? (
          <p className="loading">Reading branches…</p>
        ) : detail ? (
          <GitList
            branches={detail.git_branches}
            selected={gitShown?.name ?? null}
            onSelect={(git) => setSel({ git })}
          />
        ) : null}
        {detail && <Containers services={detail.services} />}
      </aside>

      <main className="main">
        <div className="lead">
          <p>{detail ? lead(detail, gitShown, dbSel) : error ? `Can't read ${name}` : " "}</p>
          <div className="lead-side">
            {detail && <span className="where"><span className="nw">{detail.path}</span> · <span className="nw">{detail.mode === "proxy" ? "proxy" : "env file"}</span></span>}
            {planWaiting && <button className="text-button" onClick={onBack}>See the switch plan</button>}
            <button className="text-button" onClick={refresh} disabled={loading}>{loading ? "Reading…" : "Refresh"}</button>
          </div>
        </div>
        {error ? (
          <Sheet label={`Can't read ${name}`}>
            <h2>Can't read {name}</h2>
            <p className="problem-text"><Code text={sentence(error)} /></p>
            <p className="note">Fix it, then refresh.</p>
          </Sheet>
        ) : detail ? (
          <DbTree detail={detail} selected={dbSel?.db_name ?? null} onSelect={(db) => setSel({ db })} />
        ) : null}
      </main>
    </>
  );
}

/** The line above the sheet: the selected pair in one sentence. */
function lead(d: ProjectDetail, git: GitBranchView | null, db: DbBranchView | null): ReactNode {
  const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;
  if (git) {
    if (!git.db) return <><Db>{git.name}</Db> has no database yet; switching to it creates one</>;
    if (git.here && db && !db.active && d.active_db) {
      return <><Db>{git.name}</Db> is checked out, but the app still uses <Db>{d.active_db}</Db></>;
    }
    if (git.here) return <><Db>{git.name}</Db> is checked out and the app uses <Db>{git.db}</Db></>;
    return <><Db>{git.name}</Db> has <Db>{git.db}</Db></>;
  }
  if (db && !db.git_branch_exists) return <>The git branch for <Db>{db.db_name}</Db>, <Db>{db.git_branch}</Db>, is gone</>;
  if (db) return <><Db>{db.db_name}</Db> belongs to <Db>{db.git_branch}</Db></>;
  return <>{d.name} has no branches to show</>;
}

/** Up and down move the selection through a list of buttons with these ids. */
function arrows(ids: string[], current: string | null, select: (id: string) => void) {
  return (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const i = current === null ? -1 : ids.indexOf(current);
    const next = ids[Math.min(ids.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
    if (next !== undefined) select(next);
  };
}

function GitList({ branches, selected, onSelect }: { branches: GitBranchView[]; selected: string | null; onSelect: (name: string) => void }) {
  const focus = (n: string) => {
    onSelect(n);
    document.getElementById(`git-${n}`)?.focus();
  };
  // Keep the linked row in view when the selection comes from the tree.
  useEffect(() => {
    // The row and its commits.
    if (selected) document.getElementById(`git-${selected}`)?.parentElement?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (branches.length === 0) return <p className="loading">No local git branches yet. Commit something, then refresh.</p>;
  return (
    <ul className="list git-list" aria-label="Git branches" onKeyDown={arrows(branches.map((b) => b.name), selected, focus)}>
      {branches.map((b) => {
        const on = b.name === selected;
        return (
          <li key={b.name}>
            <button id={`git-${b.name}`} className="row-button git-row" aria-current={on ? "true" : undefined} onClick={() => onSelect(b.name)}>
              <span className="name db">{b.name}</span>
              <span className={`state${b.db ? "" : " problem"}`}>{gitState(b)}</span>
            </button>
            {on && b.commits.length > 0 && (
              <div className="commits">
                <h3>{b.commits.length === 1 ? "Last commit" : `Last ${b.commits.length} commits`}</h3>
                <ol>
                  {b.commits.map((c) => (
                    <li key={c.id}>
                      <span className="subject" title={c.subject}>{c.subject}</span>
                      <span className="commit-meta"><span className="db">{c.id}</span> · {ago(c.time)}</span>
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function gitState(b: GitBranchView): ReactNode {
  const db = b.db ? <span className="db">{b.db}</span> : null;
  const where = b.here ? "Checked out here" : b.worktree ? <>Checked out in <span className="db">{b.worktree}</span></> : null;
  if (where) return join([where, db ? <>uses {db}</> : "no database yet"]);
  return db ? <>Uses {db}</> : "No database yet";
}

/** Items joined with " · ". */
function join(parts: ReactNode[]): ReactNode {
  return parts.map((p, i) => (
    <Fragment key={i}>
      {i > 0 && " · "}
      {p}
    </Fragment>
  ));
}

function Containers({ services }: { services: ServiceView[] }) {
  return (
    <div className="disk containers">
      <h3>Containers</h3>
      <ul>
        {services.map((s) => (
          <li key={s.name} data-running={s.running === null ? "unknown" : String(s.running)}>
            <i aria-hidden="true" />
            <span className="db">{s.name}</span>
            <span className="svc-state">
              {s.running === null ? "Unknown" : s.running ? "Running" : "Stopped"}
              {s.database && " · the database"}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

interface Node {
  b: DbBranchView;
  children: Node[];
}

/** Parent → child, oldest first. A branch whose parent is gone becomes a root. */
function tree(branches: DbBranchView[]): Node[] {
  const nodes = new Map(branches.map((b) => [b.db_name, { b, children: [] as Node[] }]));
  const roots: Node[] = [];
  for (const n of nodes.values()) {
    const parent = n.b.parent ? nodes.get(n.b.parent) : undefined;
    (parent ? parent.children : roots).push(n);
  }
  return roots;
}

function flatten(nodes: Node[]): string[] {
  return nodes.flatMap((n) => [n.b.db_name, ...flatten(n.children)]);
}

function DbTree({ detail, selected, onSelect }: { detail: ProjectDetail; selected: string | null; onSelect: (db: string) => void }) {
  const roots = tree(detail.db_branches);
  const focus = (db: string) => {
    onSelect(db);
    document.getElementById(`db-${db}`)?.focus();
  };
  useEffect(() => {
    if (selected) document.getElementById(`db-${selected}`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const d = detail;
  const snapshots = d.db_branches.reduce((n, b) => n + b.snapshots.length, 0);
  const noDb = d.git_branches.filter((b) => !b.db).length;
  const gone = d.db_branches.filter((b) => !b.git_branch_exists).length;
  const queries = [...(d.problem ? [`${d.problem}. Until then, whether each database still exists is unknown.`] : []), ...d.warnings];

  return (
    <Sheet label={`Database branches of ${d.name}`}>
      <h2>Database branches</h2>
      <Queries warnings={queries} />
      {roots.length === 0 ? (
        <p className="problem-text tree-empty">
          <Code text="No database branches yet. Switch to a git branch (or run `sidetrack switch`) and sidetrack creates one." />
        </p>
      ) : (
        <ul className="tree" role="tree" aria-label="Database branches" onKeyDown={arrows(flatten(roots), selected, focus)}>
          {roots.map((n) => <TreeNode key={n.b.db_name} n={n} level={1} selected={selected} onSelect={onSelect} />)}
        </ul>
      )}
      <Slug
        className="standalone tree-slug"
        items={[
          d.name,
          `${d.db_branches.length} db ${d.db_branches.length === 1 ? "branch" : "branches"}`,
          plural(snapshots, "snapshot"),
          noDb > 0 && `${noDb} git ${noDb === 1 ? "branch" : "branches"} without a database`,
          gone > 0 && `${gone} without a git branch`,
        ]}
      />
    </Sheet>
  );
}

function TreeNode({ n, level, selected, onSelect }: { n: Node; level: number; selected: string | null; onSelect: (db: string) => void }) {
  const b = n.b;
  const on = b.db_name === selected;
  return (
    <li role="treeitem" aria-level={level} aria-selected={on} aria-expanded={n.children.length > 0 ? true : undefined}>
      <button id={`db-${b.db_name}`} className="node" aria-current={on ? "true" : undefined} onClick={() => onSelect(b.db_name)} tabIndex={on || (selected === null && level === 1) ? 0 : -1}>
        <span className="node-head">
          <span className="node-name db">{b.db_name}</span>
          {b.size_bytes !== null && b.size_bytes > 0 && <span className="node-size">{bytes(b.size_bytes)}</span>}
        </span>
        <span className="node-state">{dbState(b)}</span>
      </button>
      {b.snapshots.length > 0 && (
        <ul className="snaps" aria-label={`Snapshots of ${b.db_name}`}>
          {b.snapshots.map((s) => (
            <li key={s.db_name}>
              {join([
                <span className="snap-name">Snapshot {s.seq}{s.label && <>, “{s.label}”</>}</span>,
                <span className="db">{s.db_name}</span>,
                s.size_bytes ? bytes(s.size_bytes) : null,
                ago(s.created_at),
                s.on_server === false ? <strong>not on the server</strong> : null,
              ].filter(Boolean))}
            </li>
          ))}
        </ul>
      )}
      {n.children.length > 0 && (
        <ul role="group" className="children">
          {n.children.map((c) => <TreeNode key={c.b.db_name} n={c} level={level + 1} selected={selected} onSelect={onSelect} />)}
        </ul>
      )}
    </li>
  );
}

function dbState(b: DbBranchView): ReactNode {
  const branch = <span className="db">{b.git_branch}</span>;
  return join([
    b.git_branch_exists ? <>For {branch}</> : <strong>Its git branch, {branch}, is gone</strong>,
    b.active && <strong>the app uses it</strong>,
    b.worktree && <strong>used in <span className="db">{b.worktree}</span></strong>,
    b.on_server === false && <strong>not on the server</strong>,
    `${b.adopted ? "adopted" : "created"} ${ago(b.created_at)}`,
  ].filter(Boolean));
}
