import { Fragment, useCallback, useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import { loadDetail, planOp } from "./api";
import { ago, bytes, plural } from "./format";
import { Proof } from "./Proof";
import { Problem, Queries, Sheet } from "./Sheets";
import { Code, sentence, Slug, Units } from "./text";
import type { DbBranchView, GitBranchView, Op, Plan, ProjectDetail, ServiceView } from "./types";

/** What's selected. A git branch and its DB branch are selected together. */
type Selection = { git: string } | { db: string };

/** Where an action lives: a DB node, a git row, or the whole project (gc). */
type Place = `db:${string}` | `git:${string}` | "project";

/** An inline field that's open: a snapshot label or a rename target. */
type Field = { kind: "snapshot" | "rename"; db: string } | null;

/** The plan on screen instead of the tree, and the operation it came from. */
interface Shown {
  op: Op;
  plan: Plan;
  title: string;
}

/** What the planner said no to, shown where the action was chosen. */
interface Refusal {
  place: Place;
  message: string;
}

/** The functions an action needs: plan an operation, open or close a field. */
interface Actions {
  /** The action being planned right now, by `place` + label. */
  planning: string | null;
  refusal: Refusal | null;
  field: Field;
  setField: (f: Field) => void;
  show: (place: Place, key: string, op: Op, title: string) => void;
}

const message = (e: unknown) => sentence(e instanceof Error ? e.message : String(e));

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
  const [field, setField] = useState<Field>(null);
  const [planning, setPlanning] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  const [shown, setShown] = useState<Shown | null>(null);
  const [applying, setApplying] = useState(false);
  const busy = useRef(false);
  const shownRef = useRef(shown);
  shownRef.current = shown;

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

  // Like All projects: read again whenever the window comes back to the front,
  // unless a plan is on screen (it stays as shown until it's closed).
  useEffect(() => {
    refresh();
    const onFocus = () => {
      if (!shownRef.current) refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  /** Back to the tree from a proof: read the project again, focus the selection. */
  const closeProof = useCallback(async () => {
    setShown(null);
    setApplying(false);
    await refresh();
    requestAnimationFrame(() => (document.querySelector(".node[aria-current='true']") as HTMLElement | null)?.focus());
  }, [refresh]);

  // Escape steps back one level: proof → tree, open field → closed, tree → all projects.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (shown) {
        if (!applying) closeProof();
      } else if (field) {
        setField(null);
        setRefusal(null);
      } else {
        onBack();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack, shown, applying, field, closeProof]);

  const show = useCallback(
    async (place: Place, key: string, op: Op, title: string) => {
      setPlanning(`${place}/${key}`);
      setRefusal(null);
      try {
        const plan = await planOp(root, op);
        setShown({ op, plan, title });
        setField(null);
      } catch (e) {
        setRefusal({ place, message: message(e) });
      } finally {
        setPlanning(null);
      }
    },
    [root],
  );
  const actions: Actions = { planning, refusal, field, setField, show };

  // Picking another branch sets an unapplied plan aside; a running one stays.
  const select = (s: Selection) => {
    if (applying) return;
    setSel(s);
    setShown(null);
    setField(null);
    setRefusal(null);
  };

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
          <button className="text-button" onClick={onBack} disabled={applying}>Back to all projects</button>
        </div>
        <h1 className="project-title">{name}</h1>
        {detail === null && !error ? (
          <p className="loading">Reading branches…</p>
        ) : detail ? (
          <GitList
            branches={detail.git_branches}
            selected={gitShown?.name ?? null}
            onSelect={(git) => select({ git })}
            extra={(b) => (!b.db && !detail.problem ? <CreateField b={b} detail={detail} actions={actions} /> : null)}
          />
        ) : null}
        {detail && <Containers services={detail.services} />}
      </aside>

      <main className="main">
        <div className="lead">
          <p>{detail ? lead(detail, gitShown, dbSel, shown?.op) : error ? `Can't read ${name}` : " "}</p>
          <div className="lead-side">
            {detail && <span className="where"><Units items={[detail.path, detail.mode === "proxy" ? "proxy" : "env file"]} /></span>}
            {planWaiting && <button className="text-button" onClick={onBack} disabled={applying}>See the switch plan</button>}
            {detail && !detail.problem && (
              <button
                className="text-button"
                onClick={() => show("project", "gc", { op: "gc" }, `Clean up ${detail.name}`)}
                disabled={shown !== null || planning !== null}
              >
                {planning === "project/gc" ? "Planning…" : "Clean up"}
              </button>
            )}
            <button className="text-button" onClick={refresh} disabled={loading || shown !== null}>{loading ? "Reading…" : "Refresh"}</button>
          </div>
        </div>
        {shown && detail ? (
          shown.plan.steps.length === 0 ? (
            <Sheet label="Nothing to clean up" title="Nothing to clean up" inRegister>
              <p className="problem-text">Every DB branch still has its git branch, and none is merged or missing.</p>
              {shown.plan.notes.map((n) => <p key={n} className="note"><Code text={sentence(n)} /></p>)}
              <div className="row standalone-row"><button className="secondary" autoFocus onClick={closeProof}>Back to the branches</button></div>
            </Sheet>
          ) : (
            <Proof
              key={JSON.stringify(shown.op)}
              root={root}
              name={detail.name}
              op={shown.op}
              plan={shown.plan}
              title={shown.title}
              disk={detail.disk}
              freed={freedBytes(shown.plan, detail)}
              autoFocus
              onStart={() => setApplying(true)}
              onClose={closeProof}
            />
          )
        ) : error ? (
          <Sheet label={`Can't read ${name}`} title={`Can't read ${name}`}>
            <Problem text={error} />
            <p className="note">Fix it, then refresh.</p>
          </Sheet>
        ) : detail ? (
          <DbTree detail={detail} selected={dbSel?.db_name ?? null} onSelect={(db) => select({ db })} actions={actions} />
        ) : null}
      </main>
    </>
  );
}

/**
 * The line above the sheet: the selected pair in one sentence. Over a plan for
 * the whole project (gc) it says why there's something to clean up instead.
 */
function lead(d: ProjectDetail, git: GitBranchView | null, db: DbBranchView | null, op?: Op): ReactNode {
  const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;
  if (op?.op === "gc") {
    const n = d.db_branches.length;
    const gone = d.db_branches.filter((b) => !b.git_branch_exists).length;
    const branches = `${n} DB ${n === 1 ? "branch" : "branches"}`;
    if (gone === 0) return <>{d.name} has {branches}</>;
    return <>{gone} of {branches} {gone === 1 ? "has" : "have"} lost {gone === 1 ? "its" : "their"} git branch</>;
  }
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

function GitList({ branches, selected, onSelect, extra }: {
  branches: GitBranchView[];
  selected: string | null;
  onSelect: (name: string) => void;
  /** Shown under the selected row, after its commits. */
  extra: (b: GitBranchView) => ReactNode;
}) {
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
            {on && extra(b)}
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

function DbTree({ detail, selected, onSelect, actions }: { detail: ProjectDetail; selected: string | null; onSelect: (db: string) => void; actions: Actions }) {
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
  const queries = [
    ...(actions.refusal?.place === "project" ? [actions.refusal.message] : []),
    ...(d.problem ? [`${d.problem}. Until then, whether each database still exists is unknown.`] : []),
    ...d.warnings,
  ];
  // Changing a database needs the server; without it, the tree is read-only.
  const nodeActions = (b: DbBranchView) => (d.problem ? null : <NodeActions b={b} detail={d} actions={actions} />);

  return (
    <Sheet label={`Database branches of ${d.name}`} title="Database branches">
      <Queries warnings={queries} />
      {roots.length === 0 ? (
        <p className="problem-text tree-empty">
          <Code text="No database branches yet. Switch to a git branch (or run `sidetrack switch`) and sidetrack creates one." />
        </p>
      ) : (
        <ul className="tree" role="tree" aria-label="Database branches" onKeyDown={arrows(flatten(roots), selected, focus)}>
          {roots.map((n) => <TreeNode key={n.b.db_name} n={n} level={1} selected={selected} onSelect={onSelect} actions={nodeActions} />)}
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

function TreeNode({ n, level, selected, onSelect, actions }: {
  n: Node;
  level: number;
  selected: string | null;
  onSelect: (db: string) => void;
  actions: (b: DbBranchView) => ReactNode;
}) {
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
      {on && actions(b)}
      {n.children.length > 0 && (
        <ul role="group" className="children">
          {n.children.map((c) => <TreeNode key={c.b.db_name} n={c} level={level + 1} selected={selected} onSelect={onSelect} actions={actions} />)}
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

// ---------------------------------------------------------------- actions

/** Bytes the plan's drops free, from the sizes the project view knows. */
function freedBytes(plan: Plan, d: ProjectDetail): number {
  const size = new Map<string, number>();
  for (const b of d.db_branches) {
    size.set(b.db_name, b.size_bytes ?? 0);
    for (const s of b.snapshots) size.set(s.db_name, s.size_bytes ?? 0);
  }
  return plan.steps.reduce((n, s) => n + (s.type === "drop_database" ? size.get(s.name) ?? 0 : 0), 0);
}

/** A text button that plans `op`, reading "Planning…" while it does. */
function PlanButton({ actions, place, label, op, title, children }: {
  actions: Actions;
  place: Place;
  label: string;
  op: Op;
  title: string;
  children: ReactNode;
}) {
  const key = `${place}/${label}`;
  return (
    <button type="button" className="text-button" disabled={actions.planning !== null} onClick={() => actions.show(place, label, op, title)}>
      {actions.planning === key ? "Planning…" : children}
    </button>
  );
}

/** What can be done to the selected DB branch, under its node. */
function NodeActions({ b, detail, actions }: { b: DbBranchView; detail: ProjectDetail; actions: Actions }) {
  const place: Place = `db:${b.db_name}`;
  const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;
  if (b.worktree) {
    return (
      <div className="actions">
        <p className="action-note">The app in <Db>{b.worktree}</Db> uses it, so change it from that worktree.</p>
      </div>
    );
  }
  const parent = b.parent ? detail.db_branches.find((p) => p.db_name === b.parent && p.on_server !== false) : undefined;
  const snaps = b.snapshots.filter((s) => s.on_server !== false);
  const field = actions.field?.db === b.db_name ? actions.field.kind : null;
  const refusal = actions.refusal?.place === place ? actions.refusal.message : null;
  const open = (kind: "snapshot" | "rename") => actions.setField(field === kind ? null : { kind, db: b.db_name });
  const busy = actions.planning !== null;

  return (
    <div className="actions" aria-label={`Actions for ${b.db_name}`} role="group">
      <div className="action-row">
        {b.active && (
          <button type="button" className="text-button" aria-expanded={field === "snapshot"} disabled={busy} onClick={() => open("snapshot")}>
            Snapshot
          </button>
        )}
        {b.active && parent && (
          <PlanButton actions={actions} place={place} label="reset-parent" op={{ op: "reset", to: null, parent: true }} title={`Reset ${b.db_name} to ${parent.db_name}`}>
            Reset to <Db>{parent.db_name}</Db>
          </PlanButton>
        )}
        {b.active && snaps.map((s) => (
          <PlanButton key={s.db_name} actions={actions} place={place} label={`reset-${s.db_name}`} op={{ op: "reset", to: s.db_name, parent: false }} title={`Reset ${b.db_name} to snapshot ${s.seq}`}>
            Reset to snapshot {s.seq}
          </PlanButton>
        ))}
        <button type="button" className="text-button" aria-expanded={field === "rename"} disabled={busy} onClick={() => open("rename")}>
          Rename
        </button>
        {!b.active && (
          <PlanButton actions={actions} place={place} label="delete" op={{ op: "delete", name: b.db_name }} title={`Delete ${b.db_name}`}>
            Delete
          </PlanButton>
        )}
      </div>
      {b.active && <p className="action-note">The app uses it, so it can't be deleted. Switch to another branch first.</p>}
      {field === "snapshot" && (
        <InlineField
          id={`snap-${b.db_name}`}
          label="Label (optional)"
          initial=""
          code={false}
          planning={actions.planning === `${place}/snapshot`}
          disabled={busy}
          onCancel={() => actions.setField(null)}
          onSubmit={(v) => actions.show(place, "snapshot", { op: "snapshot", label: v || null }, `Snapshot ${b.db_name}`)}
        />
      )}
      {field === "rename" && (
        <InlineField
          id={`rename-${b.db_name}`}
          label="New git branch name"
          initial={b.git_branch}
          code
          planning={actions.planning === `${place}/rename`}
          disabled={busy}
          invalid={(v) => v === "" || v === b.git_branch}
          onCancel={() => actions.setField(null)}
          onSubmit={(v) => actions.show(place, "rename", { op: "rename", old: b.db_name, new: v }, `Rename ${b.db_name}`)}
        />
      )}
      {refusal && <p className="query"><strong>Query</strong> <Code text={refusal} /></p>}
    </div>
  );
}

/** A small inline form: one labelled field, "Show the plan" and "Cancel". */
function InlineField({ id, label, initial, code, planning, disabled, invalid, onCancel, onSubmit }: {
  id: string;
  label: string;
  initial: string;
  /** Set the value in the database face (a branch name). */
  code: boolean;
  planning: boolean;
  disabled: boolean;
  invalid?: (value: string) => boolean;
  onCancel: () => void;
  onSubmit: (value: string) => void;
}) {
  const [value, setValue] = useState(initial);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);
  const v = value.trim();
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!invalid?.(v)) onSubmit(v);
  };
  return (
    <form className="inline-field" onSubmit={submit}>
      <label htmlFor={id}>{label}</label>
      <div className="field-row">
        <input
          id={id}
          className={code ? "db" : undefined}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          ref={input}
          spellCheck={false}
          autoComplete="off"
        />
        <button type="submit" className="secondary" disabled={disabled || Boolean(invalid?.(v))}>{planning ? "Planning…" : "Show the plan"}</button>
        <button type="button" className="text-button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

/** A git branch with no database: create one, from the base sidetrack picks or one you choose. */
function CreateField({ b, detail, actions }: { b: GitBranchView; detail: ProjectDetail; actions: Actions }) {
  const place: Place = `git:${b.name}`;
  const [from, setFrom] = useState("auto");
  const bases = detail.db_branches.filter((d) => d.on_server !== false);
  const refusal = actions.refusal?.place === place ? actions.refusal.message : null;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const op: Op = { op: "create", git_branch: b.name, from: from === "auto" || from === "empty" ? null : from, empty: from === "empty" };
    actions.show(place, "create", op, `Create a database for ${b.name}`);
  };
  return (
    <form className="inline-field create" onSubmit={submit}>
      <label htmlFor={`from-${b.name}`}>Create its database from</label>
      <select id={`from-${b.name}`} value={from} onChange={(e) => setFrom(e.target.value)}>
        <option value="auto">The closest match</option>
        {bases.map((d) => <option key={d.db_name} value={d.db_name}>{d.db_name}</option>)}
        <option value="empty">Nothing (start empty)</option>
      </select>
      <button type="submit" className="secondary" disabled={actions.planning !== null}>
        {actions.planning === `${place}/create` ? "Planning…" : "Show the plan"}
      </button>
      {refusal && <p className="query"><strong>Query</strong> <Code text={refusal} /></p>}
    </form>
  );
}
