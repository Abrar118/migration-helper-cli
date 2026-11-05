import { useEffect, useState, type ReactNode } from "react";
import { Canvas, type CanvasEdge, type CanvasNode, type Drop, type Zone } from "./Canvas";
import { ago, bytes } from "./format";
import { useProject, type Actions } from "./project";
import { CreateField, freedBytes, lead, NodeActions, PlanButton, tree, type Node } from "./ProjectPage";
import { Proof } from "./Proof";
import { Problem, Queries, Sheet } from "./Sheets";
import { Code, sentence } from "./text";
import { loadJson, saveJson } from "./theme";
import type { DbBranchView, Op, ProjectDetail, SnapshotView } from "./types";

export type Section = "branches" | "databases" | "containers" | "structure";

export const SECTIONS: { id: Section; label: string }[] = [
  { id: "branches", label: "Branches" },
  { id: "databases", label: "Databases" },
  { id: "containers", label: "Containers" },
  { id: "structure", label: "Structure" },
];

interface Props {
  root: string;
  section: Section;
  planWaiting: boolean;
  onSeePlan: () => void;
  /** While a plan applies, the sidebar must not navigate away. */
  onBusy: (busy: boolean) => void;
}

type Tab = "details" | "commits" | "warnings";
const PANEL = "sidetrack.panel-open";

/** One section of a project as a node canvas, with a card of actions on the
    selected node and a details panel underneath. Plans open in place of the canvas. */
export function ProjectCanvas({ root, section, planWaiting, onSeePlan, onBusy }: Props) {
  // The card opens on a click and closes on a second click, a click on the
  // empty canvas, or Escape.
  const [cardOpen, setCardOpen] = useState(false);
  const p = useProject(root, () => setCardOpen(false));
  const { detail: d, error, loading, refresh, select, shown, applying, setApplying, show, closeProof, actions, main, gitShown, dbSel } = p;
  const [picked, setPicked] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("details");
  const [panel, setPanel] = useState(() => loadJson(PANEL, true));
  useEffect(() => onBusy(applying), [applying, onBusy]);
  useEffect(() => saveJson(PANEL, panel), [panel]);
  useEffect(() => {
    setPicked(null);
    setCardOpen(false);
  }, [section]);

  const name = d?.name ?? root.split(/[\\/]/).pop() ?? root;
  const title = SECTIONS.find((s) => s.id === section)!.label;

  /** Pick a node: git and DB nodes also move the shared selection. */
  const pick = (id: string) => {
    if (applying) return;
    setCardOpen(!(cardOpen && id === current));
    setPicked(id);
    if (id.startsWith("git:")) select({ git: id.slice(4) });
    else if (id.startsWith("db:")) select({ db: id.slice(3) });
    else if (id.startsWith("snap:") && d) {
      const owner = d.db_branches.find((b) => b.snapshots.some((s) => s.db_name === id.slice(5)));
      if (owner) select({ db: owner.db_name });
      setPicked(id);
    }
  };
  // Without a pick of its own, the canvas follows the shared selection.
  const current = picked ?? (section === "branches" && gitShown ? `git:${gitShown.name}` : dbSel ? `db:${dbSel.db_name}` : null);

  const plan = (place: `db:${string}` | `git:${string}`, key: string, op: Op, t: string, node: string) => {
    setPicked(node);
    if (node.startsWith("git:")) select({ git: node.slice(4) });
    else if (node.startsWith("db:")) select({ db: node.slice(3) });
    show(place, key, op, t);
  };

  let body: ReactNode;
  if (shown && d) {
    body = (
      <div className="pane-scroll" ref={main as React.RefObject<HTMLDivElement>}>
        {shown.plan.steps.length === 0 ? (
          <Sheet label="Nothing to clean up" title="Nothing to clean up" inRegister>
            <p className="problem-text">Every DB branch still has its git branch, and none is merged or missing.</p>
            {shown.plan.notes.map((n) => <p key={n} className="note"><Code text={sentence(n)} /></p>)}
            <div className="row standalone-row"><button className="secondary" autoFocus onClick={closeProof}>Back to the canvas</button></div>
          </Sheet>
        ) : (
          <Proof
            key={JSON.stringify(shown.op)}
            root={root}
            name={d.name}
            op={shown.op}
            plan={shown.plan}
            title={shown.title}
            warnings={shown.warnings}
            disk={d.disk}
            freed={freedBytes(shown.plan, d)}
            autoFocus
            onStart={() => setApplying(true)}
            onClose={closeProof}
          />
        )}
      </div>
    );
  } else if (error) {
    body = (
      <div className="pane-scroll">
        <Sheet label={`Can't read ${name}`} title={`Can't read ${name}`}>
          <Problem text={error} />
          <p className="note">Fix it, then refresh.</p>
        </Sheet>
      </div>
    );
  } else if (!d) {
    body = <p className="loading">Reading {name}…</p>;
  } else {
    const g = graph(section, d, current, actions, plan);
    body = (
      <>
        <Canvas
          layoutKey={`${root}:${section}`}
          label={`${title} of ${d.name}`}
          nodes={g.nodes}
          edges={g.edges}
          zones={g.zones}
          dropFor={d.problem || applying ? undefined : g.dropFor}
          hint={g.hint}
          onSelect={pick}
          card={current && cardOpen ? g.card(current) : null}
          cardFor={current}
          onBackground={() => setCardOpen(false)}
          legend={g.legend}
        />
        <DetailsPanel d={d} id={current} tab={tab} onTab={setTab} open={panel} onToggle={() => setPanel((o: boolean) => !o)} />
      </>
    );
  }

  return (
    <div className="pane">
      <header className="pane-head">
        <h1>{title}</h1>
        <p className="pane-lead">{d ? lead(d, gitShown, dbSel, shown?.op) : error ? `Can't read ${name}` : ""}</p>
        <div className="pane-tools">
          {d && <span className="where">{d.path} · {d.mode === "proxy" ? "proxy" : "env file"}</span>}
          {planWaiting && <button className="btn" onClick={onSeePlan} disabled={applying}>See the switch plan</button>}
          {d && !d.problem && (
            <button className="btn" onClick={() => show("project", "gc", { op: "gc" }, `Clean up ${d.name}`)} disabled={shown !== null || actions.planning !== null}>
              {actions.planning === "project/gc" ? "Planning…" : "Clean up"}
            </button>
          )}
          <button className="btn" onClick={refresh} disabled={loading || shown !== null}>{loading ? "Reading…" : "Refresh"}</button>
        </div>
      </header>
      {actions.refusal?.place === "project" && <p className="pane-query"><strong>Query</strong> <Code text={actions.refusal.message} /></p>}
      <div className="pane-body">{body}</div>
    </div>
  );
}

// ------------------------------------------------------------------ graphs

type PlanFn = (place: `db:${string}` | `git:${string}`, key: string, op: Op, title: string, node: string) => void;

interface Graph {
  legend?: { type: string; label: string }[];
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  zones?: Zone[];
  dropFor?: (dragged: string, target: string) => Drop | null;
  hint: string;
  card: (id: string) => ReactNode;
}

function graph(section: Section, d: ProjectDetail, current: string | null, actions: Actions, plan: PlanFn): Graph {
  switch (section) {
    case "branches": return branches(d, current, actions, plan);
    case "databases": return databases(d, current, actions, plan);
    case "containers": return containers(d, current);
    case "structure": return structure(d, current);
  }
}

const Badge = ({ kind = "", children }: { kind?: string; children: ReactNode }) => <span className={`badge ${kind}`}>{children}</span>;

function dbNode(b: DbBranchView, x: number, y: number, current: string | null): CanvasNode {
  return {
    id: `db:${b.db_name}`, x, y, w: 270, kind: "db", type: "db", label: b.db_name,
    selected: current === `db:${b.db_name}`, muted: b.on_server === false,
    children: (
      <>
        <span className="cn-t"><span className="db">{b.db_name}</span>{b.active && <Badge kind="ok">app uses it</Badge>}</span>
        <span className="cn-s">
          {b.git_branch_exists ? <span className="db">{b.git_branch}</span> : <Badge kind="bad">git branch gone</Badge>}
          {b.worktree && <Badge>in a worktree</Badge>}
          {b.on_server === false && <Badge kind="bad">not on the server</Badge>}
          {b.size_bytes ? <> · {bytes(b.size_bytes)}</> : null}
        </span>
      </>
    ),
  };
}

function branches(d: ProjectDetail, current: string | null, actions: Actions, plan: PlanFn): Graph {
  const row = 104;
  const nodes: CanvasNode[] = d.git_branches.map((b, i) => ({
    id: `git:${b.name}`, x: 0, y: i * row, w: 330, kind: `git${b.db ? "" : " nodb"}`, type: "git", label: b.name,
    selected: current === `git:${b.name}`,
    children: (
      <>
        <span className="cn-t"><span className="db">{b.name}</span>{b.here && <Badge kind="accent">checked out</Badge>}</span>
        <span className="cn-s">{b.worktree ? <>In <span className="db">{b.worktree}</span></> : b.db ? "Has a database" : <Badge kind="warn">no database yet</Badge>}</span>
        {b.commits[0] && <span className="cn-m">{b.commits[0].subject} · {ago(b.commits[0].time)}</span>}
      </>
    ),
  }));
  const ordered = [
    ...d.git_branches.flatMap((g) => d.db_branches.filter((b) => b.db_name === g.db)),
    ...d.db_branches.filter((b) => !d.git_branches.some((g) => g.db === b.db_name)),
  ];
  ordered.forEach((b, i) => nodes.push(dbNode(b, 500, i * row, current)));
  const edges: CanvasEdge[] = d.git_branches.filter((g) => g.db && d.db_branches.some((b) => b.db_name === g.db)).map((g) => ({ from: `git:${g.name}`, to: `db:${g.db}`, kind: "map", label: "uses" }));
  const noDb = (id: string) => (id.startsWith("git:") ? d.git_branches.find((g) => g.name === id.slice(4) && !g.db) : undefined);
  const create = (g: string, from: string | null, empty: boolean): Drop => ({
    label: empty ? `create an empty database for ${g}` : from ? `copy ${from} for ${g}` : `create a database for ${g} from the closest match`,
    run: () => plan(`git:${g}`, "create", { op: "create", git_branch: g, from, empty }, `Create a database for ${g}`, `git:${g}`),
  });
  return {
    nodes, edges,
    legend: [{ type: "git", label: "Git branch" }, { type: "db", label: "Database" }],
    zones: [{ id: "auto", label: "Create from closest match" }, { id: "empty", label: "Create empty" }],
    dropFor: (dragged, target) => {
      const g = noDb(dragged);
      if (!g) return null;
      if (target === "auto") return create(g.name, null, false);
      if (target === "empty") return create(g.name, null, true);
      const base = target.startsWith("db:") ? d.db_branches.find((b) => b.db_name === target.slice(3) && b.on_server !== false) : undefined;
      return base ? create(g.name, base.db_name, false) : null;
    },
    hint: "Drag a git branch with no database onto a database to copy it, or onto the dock",
    card: (id) => {
      if (id.startsWith("db:")) return dbCard(d, id.slice(3), actions);
      const g = d.git_branches.find((b) => `git:${b.name}` === id);
      if (!g || g.db || d.problem) return null;
      return <CreateField b={g} detail={d} actions={actions} />;
    },
  };
}

function dbCard(d: ProjectDetail, db: string, actions: Actions): ReactNode {
  const b = d.db_branches.find((x) => x.db_name === db);
  if (!b || d.problem) return null;
  return <NodeActions b={b} detail={d} actions={actions} />;
}

function databases(d: ProjectDetail, current: string | null, actions: Actions, plan: PlanFn): Graph {
  const nodes: CanvasNode[] = [];
  const edges: CanvasEdge[] = [];
  const colW = 370, nodeH = 64, snapH = 34;
  const place = (n: Node, depth: number, y: number): number => {
    const b = n.b;
    nodes.push(dbNode(b, depth * colW, y, current));
    b.snapshots.forEach((s, k) => {
      nodes.push({
        id: `snap:${s.db_name}`, x: depth * colW + 28, y: y + nodeH + 6 + k * snapH, w: 240, kind: "snap", type: "snap", label: `Snapshot ${s.seq} of ${b.db_name}`,
        selected: current === `snap:${s.db_name}`, muted: s.on_server === false,
        children: <span className="cn-s">Snapshot {s.seq}{s.label && <> “{s.label}”</>}{s.size_bytes ? <> · {bytes(s.size_bytes)}</> : null} · {ago(s.created_at)}</span>,
      });
      edges.push({ from: `db:${b.db_name}`, to: `snap:${s.db_name}`, kind: "snap", route: "below" });
    });
    const own = nodeH + 6 + b.snapshots.length * snapH + 18;
    let cy = y;
    for (const c of n.children) {
      edges.push({ from: `db:${b.db_name}`, to: `db:${c.b.db_name}`, kind: "parent", label: "copied to" });
      cy += place(c, depth + 1, cy);
    }
    return Math.max(own, cy - y);
  };
  let y = 0;
  for (const r of tree(d.db_branches)) y += place(r, 0, y) + 12;

  const active = d.db_branches.find((b) => b.active && !b.worktree);
  const snapOf = (id: string): { s: SnapshotView; of: DbBranchView } | undefined => {
    for (const of of d.db_branches) {
      const s = of.snapshots.find((x) => `snap:${x.db_name}` === id);
      if (s) return { s, of };
    }
  };
  const dbOf = (id: string) => (id.startsWith("db:") ? d.db_branches.find((b) => b.db_name === id.slice(3)) : undefined);
  const resetTo = (source: string): Drop | null => {
    if (!active) return null;
    const snap = snapOf(source);
    if (snap && snap.s.on_server !== false) {
      const t = `Reset ${active.db_name} to snapshot ${snap.s.seq} of ${snap.of.db_name}`;
      return { label: t.toLowerCase(), danger: true, run: () => plan(`db:${active.db_name}`, "reset", { op: "reset", to: snap.s.db_name, parent: false }, t, `db:${active.db_name}`) };
    }
    const parent = dbOf(source);
    if (parent && active.parent === parent.db_name && parent.on_server !== false) {
      const t = `Reset ${active.db_name} to ${parent.db_name}`;
      return { label: t.toLowerCase(), danger: true, run: () => plan(`db:${active.db_name}`, "reset", { op: "reset", to: null, parent: true }, t, `db:${active.db_name}`) };
    }
    return null;
  };

  return {
    nodes, edges,
    legend: [{ type: "db", label: "Database branch" }, { type: "snap", label: "Snapshot" }],
    zones: [{ id: "snapshot", label: "Snapshot" }, { id: "reset", label: "Reset the app's database to it" }, { id: "delete", label: "Delete", danger: true }],
    dropFor: (dragged, target) => {
      const b = dbOf(dragged);
      const snap = snapOf(dragged);
      if (target === "snapshot") {
        return b && b === active ? { label: `snapshot ${b.db_name}`, run: () => plan(`db:${b.db_name}`, "snapshot", { op: "snapshot", label: null }, `Snapshot ${b.db_name}`, dragged) } : null;
      }
      if (target === "delete") {
        if (b && !b.active && !b.worktree) return { label: `delete ${b.db_name}`, danger: true, run: () => plan(`db:${b.db_name}`, "delete", { op: "delete", name: b.db_name }, `Delete ${b.db_name}`, dragged) };
        if (snap && !snap.of.worktree) {
          return { label: `delete snapshot ${snap.s.seq} of ${snap.of.db_name}`, danger: true, run: () => plan(`db:${snap.of.db_name}`, `snapshot-rm-${snap.s.db_name}`, { op: "snapshot_remove", name: snap.s.db_name }, `Delete snapshot ${snap.s.seq} of ${snap.of.db_name}`, `db:${snap.of.db_name}`) };
        }
        return null;
      }
      if (target === "reset" || (active && target === `db:${active.db_name}`)) return resetTo(dragged);
      return null;
    },
    hint: "Drag the app's database to Snapshot, a snapshot or its parent onto it to reset, anything else to Delete",
    card: (id) => {
      if (id.startsWith("db:")) return dbCard(d, id.slice(3), actions);
      const snap = snapOf(id);
      if (!snap || d.problem) return null;
      const refusal = actions.refusal && (actions.refusal.place === `db:${snap.of.db_name}` || actions.refusal.place === `db:${active?.db_name}`) ? actions.refusal.message : null;
      return (
        <div className="actions">
          <div className="action-row">
            {active && snap.s.on_server !== false && (
              <PlanButton actions={actions} place={`db:${active.db_name}`} label="reset" op={{ op: "reset", to: snap.s.db_name, parent: false }} title={`Reset ${active.db_name} to snapshot ${snap.s.seq} of ${snap.of.db_name}`}>
                Reset {active.db_name} to it
              </PlanButton>
            )}
            {!snap.of.worktree && (
              <PlanButton actions={actions} place={`db:${snap.of.db_name}`} label={`snapshot-rm-${snap.s.db_name}`} op={{ op: "snapshot_remove", name: snap.s.db_name }} title={`Delete snapshot ${snap.s.seq} of ${snap.of.db_name}`}>
                Delete
              </PlanButton>
            )}
          </div>
          {refusal && <p className="query"><strong>Query</strong> <Code text={refusal} /></p>}
        </div>
      );
    },
  };
}

/** A path inside the project as `./sub/dir`; others as given. */
function inProject(d: ProjectDetail, path: string | null): string | null {
  if (!path) return path;
  const norm = (x: string) => x.replace(/\\/g, "/").replace(/\/+$/, "");
  const p = norm(path);
  for (const root of [norm(d.root), norm(d.path)]) {
    if (p === root) return ".";
    if (p.toLowerCase().startsWith(root.toLowerCase() + "/")) return "./" + p.slice(root.length + 1);
  }
  return path;
}

function containers(d: ProjectDetail, current: string | null): Graph {
  const nodes: CanvasNode[] = [];
  const edges: CanvasEdge[] = [];
  const row = 92;
  const sel = (id: string) => current === id;
  const left = new Map<string, number>();
  let leftY = 0, rightY = 0;
  d.services.forEach((s, i) => {
    const y = i * row;
    const id = `svc:${s.name}`;
    const state = s.running === null ? "unknown" : s.running ? "running" : "stopped";
    nodes.push({
      id, x: 330, y, w: 260, kind: `svc ${state}`, type: s.database ? "dbsvc" : "svc", label: `${s.name}, ${state}`, selected: sel(id),
      children: (
        <>
          <span className="cn-t"><i className={`dot ${state}`} aria-hidden="true" />{s.name}{s.database && <Badge>database</Badge>}{s.routed && <Badge kind="accent">reads the DB name</Badge>}</span>
          <span className="cn-s">{state === "unknown" ? "State unknown" : state === "running" ? "Running" : "Stopped"}</span>
        </>
      ),
    });
    const src = s.image ? `img:${s.image}` : s.build ? `build:${s.build}` : null;
    if (src) {
      if (!left.has(src)) {
        const ly = Math.max(y, leftY);
        left.set(src, ly);
        leftY = ly + 76;
        nodes.push({
          id: src, x: 0, y: ly, w: 250, kind: s.image ? "img" : "build", type: s.image ? "image" : "source", label: s.image ?? s.build ?? "", selected: sel(src),
          children: <><span className="cn-t">{s.image ? "Image" : "Built from"}</span><span className="cn-s db">{s.image ?? inProject(d, s.build)}</span></>,
        });
      }
      edges.push({ from: src, to: id, kind: "runs", label: s.image ? "runs as" : "built into" });
    }
    for (const v of s.volumes) {
      const vid = `vol:${v.source ?? v.target}`;
      if (!nodes.some((n) => n.id === vid)) {
        const vy = Math.max(y, rightY);
        rightY = vy + 84;
        nodes.push({
          id: vid, x: 780, y: vy, w: 260, kind: "vol", type: v.kind === "bind" ? "folder" : "volume", label: v.source ?? v.target ?? "volume", selected: sel(vid),
          children: <><span className="cn-t">{v.kind === "bind" ? "Folder" : "Volume"}</span><span className="cn-s"><span className="db">{inProject(d, v.source) ?? "anonymous"}</span>{v.target && <> → <span className="db">{v.target}</span></>}</span></>,
        });
      }
      edges.push({ from: id, to: vid, kind: "mounts", label: "mounts" });
    }
    for (const dep of s.depends_on) if (d.services.some((x) => x.name === dep) && (sel(id) || sel(`svc:${dep}`))) edges.push({ from: id, to: `svc:${dep}`, kind: "dep", route: "loop", label: "needs" });
  });
  const db = d.services.find((s) => s.database);
  if (db) {
    const y = Math.max(rightY, 0);
    nodes.push({
      id: "dbs", x: 780, y, w: 260, kind: "dbs", type: "db", label: "Databases on the server", selected: sel("dbs"),
      children: (
        <>
          <span className="cn-t">{d.db_branches.length} database {d.db_branches.length === 1 ? "branch" : "branches"}</span>
          <span className="cn-s">The app uses <span className="db">{d.active_db ?? "?"}</span></span>
          {d.services.some((x) => x.routed) && <span className="cn-s">Read by {d.services.filter((x) => x.routed).map((x) => x.name).join(", ")} {d.mode === "proxy" ? "through the proxy" : <>via <span className="db">{d.structure.routing_variable}</span></>}</span>}
        </>
      ),
    });
    edges.push({ from: `svc:${db.name}`, to: "dbs", kind: "serves", label: "holds" });
    for (const s of d.services.filter((x) => x.routed)) edges.push({ from: `svc:${s.name}`, to: "dbs", kind: "reads", label: d.mode === "proxy" ? "via proxy" : `reads ${d.structure.routing_variable}` });
  }
  const legend = [
    { type: "image", label: "Image" }, { type: "source", label: "Source folder (built)" }, { type: "dbsvc", label: "Database container" },
    { type: "svc", label: "Container" }, { type: "volume", label: "Volume" }, { type: "folder", label: "Mounted folder" }, { type: "db", label: "Databases" },
  ];
  return { legend, nodes, edges, hint: "Images and builds on the left, services in the middle, volumes and databases on the right. Select a service to see what it depends on.", card: () => null };
}

function structure(d: ProjectDetail, current: string | null): Graph {
  const s = d.structure;
  const sel = (id: string) => current === id;
  const n = (id: string, x: number, y: number, t: ReactNode, sub: ReactNode, type: string): CanvasNode => ({
    id, x, y, w: 250, kind: "part", type, label: id, selected: sel(id),
    children: <><span className="cn-t">{t}</span><span className="cn-s">{sub}</span></>,
  });
  const nodes: CanvasNode[] = [
    n("repo", 0, 120, d.name, <>{d.path}{d.git_branch && <> · on <span className="db">{d.git_branch}</span></>}</>, "git"),
    n("config", 330, 120, <span className="db">sidetrack.toml</span>, `${s.engine} · ${s.routing_mode === "proxy" ? "proxy" : "env file"} routing`, "config"),
    n("compose", 670, 0, "Compose", <>{s.compose_project ?? "project name unknown"} · {s.compose_files.length ? s.compose_files.join(", ") : "default files"}</>, "svc"),
    n("routing", 670, 100, "Routing", s.routing_mode === "proxy" ? <>Proxy on <span className="db">{s.listen ?? "?"}</span></> : <><span className="db">{s.routing_file}</span> · <span className="db">{s.routing_variable}</span></>, "routing"),
    n("migrations", 670, 200, "Migrations", <>{s.migrations_tool} · <span className="db">{s.migrations_command}</span></>, "migrations"),
    n("database", 670, 300, "Database", <>Service <span className="db">{s.db_service}</span> · user <span className="db">{s.db_user}</span></>, "dbsvc"),
  ];
  const edges: CanvasEdge[] = [
    { from: "repo", to: "config", kind: "part", label: "has" },
    ...["compose", "routing", "migrations", "database"].map((to) => ({ from: "config", to, kind: "part", label: "sets" })),
  ];
  s.restart.forEach((svc, i) => {
    nodes.push(n(`app:${svc}`, 1010, i * 76, svc, s.routing_mode === "proxy" ? "Connects through the proxy" : `Reads ${s.routing_variable}, restarted on a switch`, "svc"));
    edges.push({ from: "routing", to: `app:${svc}`, kind: "reads", label: s.routing_mode === "proxy" ? "proxies" : "rewrites env" });
  });
  s.migrations_dirs.forEach((dir, i) => {
    const y = Math.max(200, s.restart.length * 76) + i * 76;
    nodes.push(n(`dir:${dir}`, 1010, y, <span className="db">{dir}</span>, "Migration files, compared by name", "folder"));
    edges.push({ from: "migrations", to: `dir:${dir}`, kind: "part", label: "reads" });
  });
  nodes.push(n("active", 1010, Math.max(300, (s.restart.length + s.migrations_dirs.length) * 76 + 120), <span className="db">{d.active_db ?? "?"}</span>, "The database the app uses", "db"));
  edges.push({ from: "database", to: "active", kind: "reads", label: "app uses" });
  if (s.main_branch) nodes.push(n("main", 0, 230, <span className="db">{s.main_branch}</span>, "Main branch, the default base", "git"));
  if (s.main_branch) edges.push({ from: "repo", to: "main", kind: "part", route: "below" });
  const legend = [
    { type: "git", label: "Git" }, { type: "config", label: "sidetrack.toml" }, { type: "svc", label: "Compose and app containers" }, { type: "routing", label: "Routing" },
    { type: "migrations", label: "Migrations" }, { type: "folder", label: "Folder" }, { type: "dbsvc", label: "Database container" }, { type: "db", label: "Database" },
  ];
  return { legend, nodes, edges, hint: "How sidetrack sees this project, read from sidetrack.toml and compose", card: () => null };
}

// ------------------------------------------------------------ details panel

function DetailsPanel({ d, id, tab, onTab, open, onToggle }: { d: ProjectDetail; id: string | null; tab: Tab; onTab: (t: Tab) => void; open: boolean; onToggle: () => void }) {
  const db = id?.startsWith("db:") ? d.db_branches.find((b) => b.db_name === id.slice(3)) : undefined;
  const git = id?.startsWith("git:") ? d.git_branches.find((b) => b.name === id.slice(4)) : db?.git_branch_exists ? d.git_branches.find((b) => b.name === db.git_branch) : undefined;
  const warnings = [...(d.problem ? [`${d.problem}. Until then, whether each database still exists is unknown.`] : []), ...d.warnings];
  const tabs: { id: Tab; label: string }[] = [
    { id: "details", label: "Details" },
    { id: "commits", label: git ? `Commits ${git.commits.length}` : "Commits" },
    { id: "warnings", label: warnings.length ? `Warnings ${warnings.length}` : "Warnings" },
  ];
  return (
    <section className={`panel${open ? "" : " closed"}`} aria-label="Details">
      <div className="panel-tabs" role="tablist">
        {tabs.map((t) => (
          <button key={t.id} role="tab" aria-selected={open && tab === t.id} onClick={() => { onTab(t.id); if (!open) onToggle(); }}>{t.label}</button>
        ))}
        <button className="panel-toggle" onClick={onToggle} aria-expanded={open}>{open ? "Hide panel" : "Show panel"}</button>
      </div>
      {open && (
        <div className="panel-body">
          {tab === "details" && <Details d={d} id={id} />}
          {tab === "commits" && (git ? (
            <ol className="commit-list">
              {git.commits.map((c) => <li key={c.id}><span>{c.subject}</span><small><span className="db">{c.id}</span> · {ago(c.time)}</small></li>)}
            </ol>
          ) : <p className="note">Select a git branch, or a database whose git branch still exists.</p>)}
          {tab === "warnings" && (warnings.length ? <Queries warnings={warnings} /> : <p className="note">Nothing needs a look.</p>)}
        </div>
      )}
    </section>
  );
}

function Kv({ rows }: { rows: [string, ReactNode][] }) {
  return <dl className="kv">{rows.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>;
}

function Details({ d, id }: { d: ProjectDetail; id: string | null }) {
  const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;
  if (!id) return <p className="note">Select a node to see its details.</p>;
  if (id.startsWith("db:")) {
    const b = d.db_branches.find((x) => x.db_name === id.slice(3));
    if (!b) return null;
    return (
      <Kv rows={[
        ["Database", <Db>{b.db_name}</Db>],
        ["Git branch", <><Db>{b.git_branch}</Db>{!b.git_branch_exists && " (gone)"}</>],
        ["Copied from", b.parent ? <Db>{b.parent}</Db> : b.adopted ? "Adopted at init" : "Created empty"],
        ["Size", b.size_bytes ? bytes(b.size_bytes) : "Unknown"],
        ["On the server", b.on_server === null ? "Unknown" : b.on_server ? "Yes" : "No"],
        ["Created", ago(b.created_at)],
        ["Last used", ago(b.last_used_at)],
        ["Snapshots", b.snapshots.length ? b.snapshots.map((s) => `${s.seq}${s.label ? ` “${s.label}”` : ""}`).join(", ") : "None"],
        ...(b.worktree ? [["Used in", <Db>{b.worktree}</Db>] as [string, ReactNode]] : []),
      ]} />
    );
  }
  if (id.startsWith("git:")) {
    const g = d.git_branches.find((x) => x.name === id.slice(4));
    if (!g) return null;
    return <Kv rows={[["Git branch", <Db>{g.name}</Db>], ["Database", g.db ? <Db>{g.db}</Db> : "None yet"], ["Checked out", g.here ? "Here" : g.worktree ? <Db>{g.worktree}</Db> : "No"], ["Last commit", g.commits[0] ? `${g.commits[0].subject} · ${ago(g.commits[0].time)}` : "None"]]} />;
  }
  if (id.startsWith("snap:")) {
    for (const b of d.db_branches) {
      const s = b.snapshots.find((x) => x.db_name === id.slice(5));
      if (s) return <Kv rows={[["Snapshot", <Db>{s.db_name}</Db>], ["Of", <Db>{b.db_name}</Db>], ["Label", s.label ?? "None"], ["Size", s.size_bytes ? bytes(s.size_bytes) : "Unknown"], ["Taken", ago(s.created_at)]]} />;
    }
    return null;
  }
  if (id.startsWith("svc:")) {
    const s = d.services.find((x) => x.name === id.slice(4));
    if (!s) return null;
    return (
      <Kv rows={[
        ["Service", <Db>{s.name}</Db>],
        ["State", s.running === null ? "Unknown" : s.running ? "Running" : "Stopped"],
        ["Image", s.image ? <Db>{s.image}</Db> : s.build ? <>Built from <Db>{s.build}</Db></> : "Unknown"],
        ["Depends on", s.depends_on.length ? s.depends_on.join(", ") : "Nothing"],
        ["Volumes", s.volumes.length ? s.volumes.map((v) => `${v.source ?? "anonymous"} → ${v.target ?? "?"}`).join(", ") : "None"],
        ["Database", s.database ? "This is the database service" : s.routed ? <>Reads <Db>{d.structure.routing_variable}</Db>, restarted on a switch</> : "Not touched by sidetrack"],
      ]} />
    );
  }
  const s = d.structure;
  const rows: Record<string, [string, ReactNode][]> = {
    repo: [["Path", <Db>{d.path}</Db>], ["Checked out", d.git_branch ? <Db>{d.git_branch}</Db> : "Detached HEAD"], ["Main branch", s.main_branch ?? "Not set"]],
    config: [["Engine", s.engine], ["Routing", s.routing_mode], ["Database service", <Db>{s.db_service}</Db>]],
    compose: [["Project", s.compose_project ?? "Unknown"], ["Files", s.compose_files.length ? s.compose_files.join(", ") : "Compose defaults"]],
    routing: s.routing_mode === "proxy"
      ? [["Mode", "Proxy"], ["Listens on", <Db>{s.listen ?? "?"}</Db>], ["Restarts", s.restart.join(", ") || "Nothing"]]
      : [["Mode", "Env file"], ["File", <Db>{s.routing_file}</Db>], ["Variable", <Db>{s.routing_variable}</Db>], ["Restarts", s.restart.join(", ") || "Nothing"]],
    migrations: [["Tool", s.migrations_tool], ["Command", <Db>{s.migrations_command}</Db>], ["Folders", s.migrations_dirs.join(", ")], ["Seed", s.seed ? <Db>{s.seed}</Db> : "None"]],
    database: [["Service", <Db>{s.db_service}</Db>], ["User", <Db>{s.db_user}</Db>], ["Engine", s.engine]],
    dbs: [["Database branches", String(d.db_branches.length)], ["The app uses", <Db>{d.active_db ?? "?"}</Db>]],
    active: [["The app uses", <Db>{d.active_db ?? "?"}</Db>]],
  };
  const r = rows[id] ?? (id.includes(":") ? [["Name", <Db>{id.slice(id.indexOf(":") + 1)}</Db>]] as [string, ReactNode][] : null);
  return r ? <Kv rows={r} /> : null;
}
