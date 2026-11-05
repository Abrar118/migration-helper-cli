import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { AddProject } from "./AddProject";
import { inApp, loadOverview, onCheckout, pickFolder } from "./api";
import { gb, plural } from "./format";
import { Proof } from "./Proof";
import { ProjectList } from "./ProjectList";
import { ProjectCanvas, SECTIONS, type Section } from "./ProjectCanvas";
import { ProjectPage } from "./ProjectPage";
import { EmptySheet, InSyncSheet, LoadErrorSheet, ProblemSheet, Sheet } from "./Sheets";
import { createsDatabase, pendingOnly, target } from "./steps";
import { LookContext } from "./look";
import { Units } from "./text";
import { applyLook, applyTheme, loadCollapsed, loadLook, loadTheme, saveCollapsed, type Look, type Theme } from "./theme";
import type { Overview, Plan, ProjectView } from "./types";

type View = "projects" | "settings";

export function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useState<View>("projects");
  const [theme, setTheme] = useState<Theme>(loadTheme);
  // The project open in the project view, if any.
  const [open, setOpen] = useState<string | null>(null);
  // A proof being applied (or just applied) stays on screen until it's closed,
  // even if a refresh replaces the project's plan.
  const [pinned, setPinned] = useState<{ root: string; plan: Plan } | null>(null);
  // Proofs set aside with "Not now", until the next refresh.
  const [setAside, setSetAside] = useState<Set<string>>(new Set());
  // A folder being added (Add project), until it's added or cancelled.
  const [adding, setAdding] = useState<string | null>(null);
  // The column folded down to a rail with only its toggle.
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [look, setLook] = useState<Look>(loadLook);
  // Canvas look: which part of the selected project the pane shows.
  const [section, setSection] = useState<Section | "overview">("overview");
  // Canvas look: a plan from a project canvas is applying, so don't navigate away.
  const [canvasBusy, setCanvasBusy] = useState(false);
  const canvasBusyRef = useRef(canvasBusy);
  canvasBusyRef.current = canvasBusy;
  const busy = useRef(false);
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;
  const openRef = useRef(open);
  openRef.current = open;

  // Asked to refresh while a read was running: that read may predate the
  // change (a checkout the hook just reported), so read once more after it.
  const again = useRef(false);

  const refresh = useCallback(async () => {
    if (busy.current) {
      again.current = true;
      return;
    }
    busy.current = true;
    setLoading(true);
    try {
      let o = await loadOverview();
      while (again.current) {
        again.current = false;
        o = await loadOverview();
      }
      setOverview(o);
      setLoadError(null);
      setSetAside(new Set());
      setSelected((cur) =>
        cur && o.projects.some((p) => p.root === cur)
          ? cur
          : (o.projects.find((p) => p.plan) ?? o.projects[0])?.root ?? null,
      );
    } catch (e) {
      setLoadError(String(e));
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => saveCollapsed(collapsed), [collapsed]);

  useEffect(() => applyLook(look), [look]);

  // Ctrl+B (⌘B on a Mac) folds the column, as in most editors. Not while typing.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== "b") return;
      if (e.target instanceof Element && e.target.closest("input, textarea, select, [contenteditable]")) return;
      e.preventDefault();
      setCollapsed((c) => !c);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // State lives in git, Docker and env files, which change outside the app:
  // read it again whenever the window comes back to the front.
  useEffect(() => {
    refresh();
    const onFocus = () => {
      if (!pinnedRef.current) refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // A checkout in a GUI client left a project needing a switch: bring up its
  // proof, unless a plan is being applied or a project is open (there, the
  // refresh adds "See the switch plan" to that project's lead).
  useEffect(
    () =>
      onCheckout((root) => {
        if (pinnedRef.current || canvasBusyRef.current) return;
        if (!openRef.current) {
          setView("projects");
          setSelected(root);
          setSection("overview");
        }
        refresh();
      }),
    [refresh],
  );

  const startAdd = useCallback(async () => {
    const dir = await pickFolder();
    if (!dir) return;
    setOpen(null);
    setView("projects");
    setAdding(dir);
  }, []);

  const back = useCallback(() => {
    setOpen(null);
    refresh();
  }, [refresh]);

  const projects = overview?.projects ?? [];
  const project = projects.find((p) => p.root === selected) ?? null;
  const disk = overview?.disk ?? null;

  const toggle = (
  <button
    className="col-toggle"
    aria-controls="column"
    aria-expanded={!collapsed}
    aria-label={collapsed ? "Expand the sidebar" : "Collapse the sidebar"}
    title={`${collapsed ? "Expand" : "Collapse"} the sidebar (${shortcut})`}
    onClick={() => setCollapsed((c) => !c)}
  >
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <rect x="2.5" y="3.5" width="15" height="13" />
      <line x1="7.5" y1="3.5" x2="7.5" y2="16.5" />
      {collapsed ? <polyline points="10.5,7.5 13,10 10.5,12.5" /> : <polyline points="13,7.5 10.5,10 13,12.5" />}
    </svg>
  </button>
  );

  const top = (
    <>
      <div className="top">
        {toggle}
        <span className="mark">sidetrack</span>
        <nav className="nav" aria-label="Main">
          <button aria-current={view === "projects" ? "page" : undefined} onClick={() => setView("projects")}>Projects</button>
          <button aria-current={view === "settings" ? "page" : undefined} onClick={() => setView("settings")}>Settings</button>
        </nav>
      </div>
      {!inApp && <p className="demo">Demo data. Open the sidetrack app to see your projects.</p>}
    </>
  );

  const layout = collapsed ? "collapsed" : undefined;

  if (look === "canvas") {
    const locked = pinned !== null || canvasBusy;
    const goto = (root: string, sec: Section | "overview") => {
      setSelected(root);
      setSection(sec);
      setView("projects");
      setAdding(null);
    };
    const here = (root: string) => view === "projects" && !adding && selected === root;
    return (
      <LookContext.Provider value="canvas">
      <div className="app canvas-app" data-col={layout}>
        <aside className="side" id="column">
          <div className="side-top">
            {toggle}
            <span className="mark">sidetrack</span>
          </div>
          {!inApp && <p className="demo">Demo data. Open the sidetrack app to see your projects.</p>}
          <nav className="side-nav" aria-label="Projects">
            <p className="side-group">Projects</p>
            {overview === null && !loadError && <p className="loading">Reading your projects…</p>}
            <ul>
              {projects.map((p) => (
                <li key={p.root}>
                  <button className="side-item" aria-current={here(p.root) && section === "overview" ? "page" : undefined} disabled={locked} onClick={() => goto(p.root, "overview")} title={p.path}>
                    <i className={`dot ${p.problem ? "bad" : p.plan ? "warn" : "ok"}`} aria-hidden="true" />
                    <span className="side-name">{p.name}</span>
                  </button>
                  {here(p.root) && (
                    <ul className="side-sub" aria-label={`Sections of ${p.name}`}>
                      {SECTIONS.map((sec) => (
                        <li key={sec.id}>
                          <button className="side-item" aria-current={section === sec.id ? "page" : undefined} disabled={locked} onClick={() => goto(p.root, sec.id)}>
                            {sec.label}
                            {sec.id === "databases" && <span className="count">{p.branches}</span>}
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              ))}
            </ul>
            <button className="side-item add" onClick={startAdd} disabled={locked}>Add project</button>
          </nav>
          <div className="side-foot">
            <button className="side-item" aria-current={view === "settings" ? "page" : undefined} disabled={locked} onClick={() => setView("settings")}>Settings</button>
            <p className="side-disk" title={disk?.host_limited ? "Docker Desktop's disk can only grow as far as this host drive allows" : undefined}>
              {disk ? `${gb(disk.free_bytes)} GB free of ${gb(disk.total_bytes)} GB${disk.host_limited ? " on this PC's drive" : ""}` : "Disk space shows once a database is running."}
            </p>
          </div>
        </aside>
        <main className="canvas-main">
          {view === "settings" ? (
            <div className="pane-scroll">
              <Settings theme={theme} onTheme={setTheme} look={look} onLook={setLook} registry={overview?.registry ?? "…"} />
            </div>
          ) : adding ? (
            <div className="pane-scroll">
              <AddProject key={adding} dir={adding} onPickAgain={startAdd} onCancel={() => setAdding(null)} onDone={(root) => { setAdding(null); goto(root, "overview"); refresh(); }} />
            </div>
          ) : loadError ? (
            <div className="pane-scroll"><LoadErrorSheet message={loadError} /></div>
          ) : overview && projects.length === 0 ? (
            <div className="pane-scroll"><EmptySheet registry={overview.registry} onAdd={startAdd} /></div>
          ) : project && section !== "overview" ? (
            <ProjectCanvas key={project.root} root={project.root} section={section} planWaiting={Boolean(project.plan)} onSeePlan={() => setSection("overview")} onBusy={setCanvasBusy} />
          ) : project ? (
            <div className="pane">
              <header className="pane-head">
                <h1>{project.name}</h1>
                <p className="pane-lead">{lead(project, pinned?.root === project.root)}</p>
                <div className="pane-tools">
                  <span className="where">{where(project)}</span>
                  <button className="btn" onClick={refresh} disabled={loading || pinned !== null}>{loading ? "Reading…" : "Refresh"}</button>
                </div>
              </header>
              <div className="pane-body">
                <div className="pane-scroll">
                  <ProjectSheet
                    p={project}
                    pinned={pinned?.root === project.root ? pinned.plan : null}
                    setAside={setAside.has(project.root)}
                    disk={disk}
                    onStart={(plan) => setPinned({ root: project.root, plan })}
                    onClose={(applied) => {
                      setPinned(null);
                      if (applied) refresh();
                      else setSetAside((s) => new Set(s).add(project.root));
                    }}
                  />
                </div>
              </div>
            </div>
          ) : null}
        </main>
      </div>
      </LookContext.Provider>
    );
  }

  if (open && view === "projects") {
    return (
      <div className="app" data-col={layout}>
        <ProjectPage
          key={open}
          root={open}
          top={top}
          planWaiting={Boolean(projects.find((p) => p.root === open)?.plan)}
          onBack={back}
        />
      </div>
    );
  }

  return (
    <div className="app" data-col={layout}>
      <aside className="col" id="column">
        {top}
        <h1>All projects</h1>
        {overview === null && !loadError ? (
          <p className="loading">Reading your projects…</p>
        ) : (
          <ProjectList projects={projects} selected={view === "projects" ? selected : null} onSelect={(r) => { setSelected(r); setView("projects"); }} />
        )}
        <div className="add-row">
          <button className="text-button" onClick={startAdd} disabled={pinned !== null}>Add project</button>
        </div>
        <div className="disk">
          {disk ? (
            <>
              <p className="n">{gb(disk.free_bytes)}<small>GB free</small></p>
              <p className="of">of {gb(disk.total_bytes)} GB {disk.host_limited ? "on the drive Docker Desktop keeps its disk on" : "on the Docker host"}</p>
              <div className="meter" role="img" aria-label={`${Math.round((1 - disk.free_bytes / disk.total_bytes) * 100)}% used`}>
                <i style={{ width: `${(1 - disk.free_bytes / disk.total_bytes) * 100}%` }} />
              </div>
            </>
          ) : (
            <p className="of">Disk space shows once a database is running.</p>
          )}
        </div>
      </aside>

      <main className="main">
        {view === "settings" ? (
          <Settings theme={theme} onTheme={setTheme} look={look} onLook={setLook} registry={overview?.registry ?? "…"} />
        ) : adding ? (
          <>
            <div className="lead">
              <p>Add a project</p>
            </div>
            <AddProject
              key={adding}
              dir={adding}
              onPickAgain={startAdd}
              onCancel={() => setAdding(null)}
              onDone={(root) => {
                setAdding(null);
                setSelected(root);
                refresh();
              }}
            />
          </>
        ) : (
          <>
            <div className="lead">
              <p>{project ? lead(project, pinned?.root === project.root) : loadError ? "Something went wrong" : projects.length === 0 && overview ? "Set up a project" : " "}</p>
              <div className="lead-side">
                {project && <span className="where">{where(project)}</span>}
                {project && (
                  <button className="text-button" onClick={() => setOpen(project.root)} disabled={pinned !== null}>
                    Open project
                  </button>
                )}
                <button className="text-button" onClick={refresh} disabled={loading || pinned !== null}>
                  {loading ? "Reading…" : "Refresh"}
                </button>
              </div>
            </div>
            {loadError ? (
              <LoadErrorSheet message={loadError} />
            ) : overview && projects.length === 0 ? (
              <EmptySheet registry={overview.registry} onAdd={startAdd} />
            ) : project ? (
              <ProjectSheet
                p={project}
                pinned={pinned?.root === project.root ? pinned.plan : null}
                setAside={setAside.has(project.root)}
                disk={disk}
                onStart={(plan) => setPinned({ root: project.root, plan })}
                onClose={(applied) => {
                  setPinned(null);
                  if (applied) refresh();
                  else setSetAside((s) => new Set(s).add(project.root));
                }}
              />
            ) : null}
          </>
        )}
      </main>
    </div>
  );
}

function ProjectSheet(props: {
  p: ProjectView;
  pinned: Plan | null;
  setAside: boolean;
  disk: Overview["disk"];
  onStart: (plan: Plan) => void;
  onClose: (applied: boolean) => void;
}) {
  const { p, pinned, disk } = props;
  const plan = pinned ?? (props.setAside ? null : p.plan);
  if (plan) {
    return (
      <Proof
        key={`${p.root}:${JSON.stringify(plan.steps)}`}
        root={p.root}
        name={p.name}
        op={{ op: "switch" }}
        plan={plan}
        title={pendingOnly(plan) ? `Migrate ${p.active_db ?? p.name}` : `Switch ${p.name} to ${p.git_branch ?? "this branch"}`}
        warnings={p.warnings}
        disk={disk}
        onStart={() => props.onStart(plan)}
        onClose={() => props.onClose(pinned !== null)}
      />
    );
  }
  if (p.problem) return <ProblemSheet p={p} />;
  return <InSyncSheet p={p} />;
}

const shortcut = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘B" : "Ctrl+B";

const Nw = ({ children }: { children: ReactNode }) => <span className="nw">{children}</span>;

/** The line above the sheet: what this project needs, in one sentence. */
function lead(p: ProjectView, applying: boolean): ReactNode {
  if (applying) return <>The plan for {p.name}</>;
  if (p.problem) return <>{p.name} needs attention</>;
  if (p.plan) {
    const pending = pendingOnly(p.plan);
    if (pending) return <>{p.name} has {plural(pending.length, "migration")} to&nbsp;run</>;
    return createsDatabase(p.plan)
      ? <>{p.name} needs a database for <Nw>{p.git_branch}</Nw></>
      : <>{p.name} should switch to <Nw>{target(p.plan)}</Nw></>;
  }
  return <>{p.name} is on the right database</>;
}

/** Where the project is, how it routes, and (when a plan waits) what the app uses now. */
function where(p: ProjectView): ReactNode {
  const parts: ReactNode[] = [p.path];
  if (p.mode === "proxy") parts.push(`proxy ${p.listen?.replace(/^.*:/, ":") ?? ""}`);
  else if (p.mode) parts.push("env file");
  if (p.plan && p.active_db && !pendingOnly(p.plan)) parts.push(<>the app is still on <span className="db">{p.active_db}</span></>);
  return <Units items={parts} />;
}

function Settings({ theme, onTheme, look, onLook, registry }: { theme: Theme; onTheme: (t: Theme) => void; look: Look; onLook: (l: Look) => void; registry: string }) {
  const looks: { value: Look; label: string; hint: string }[] = [
    { value: "proof", label: "Press Proof", hint: "Big type; every plan as a proof to sign off." },
    { value: "canvas", label: "Canvas", hint: "Plain type; each project's branches, databases, containers and structure as node canvases." },
  ];
  const options: { value: Theme; label: string; hint: string }[] = [
    { value: "system", label: "Match the system", hint: "Light or dark, following your OS." },
    { value: "light", label: "Light", hint: "Paper on the light table." },
    { value: "dark", label: "Dark", hint: "The light table at night." },
  ];
  return (
    <Sheet label="Settings" title="Settings">
      <fieldset className="themes">
        <legend>Look</legend>
        {looks.map((o) => (
          <label key={o.value} className="theme-option">
            <input type="radio" name="look" value={o.value} checked={look === o.value} onChange={() => onLook(o.value)} />
            <span>
              <span className="theme-name">{o.label}</span>
              <span className="theme-hint">{o.hint}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <fieldset className="themes">
        <legend>Theme</legend>
        {options.map((o) => (
          <label key={o.value} className="theme-option">
            <input type="radio" name="theme" value={o.value} checked={theme === o.value} onChange={() => onTheme(o.value)} />
            <span>
              <span className="theme-name">{o.label}</span>
              <span className="theme-hint">{o.hint}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <StepColours />
      <div className="setting">
        <h3>Registry</h3>
        <p className="db">{registry}</p>
        <p className="note">One file for every project on this machine. Set SIDETRACK_REGISTRY to use another.</p>
      </div>
    </Sheet>
  );
}

/**
 * The key to the colour bar, as a tint ramp: each step family at full ink (the
 * step has run, or will once you apply) and at the pale tint of a step still
 * waiting while a plan applies. The tint is mixed with this theme's paper.
 */
function StepColours() {
  const families: { ink: string; name: string; steps: string }[] = [
    { ink: "key", name: "Key", steps: "Stop or restart the app, hold or release its connections" },
    { ink: "cyan", name: "Cyan", steps: "Copy, create, freeze or rename a database" },
    { ink: "magenta", name: "Magenta", steps: "Point the app at another database" },
    { ink: "yellow", name: "Yellow", steps: "Run migrations or the seed" },
    { ink: "spot", name: "Red", steps: "Drop a database. Only drops are red." },
  ];
  return (
    <div className="step-colours">
      <h3>Step colours</h3>
      <p className="note">Every step on a proof takes its family's ink. Full ink: the step has run, or will once you apply. Pale: it's still waiting while the plan applies.</p>
      <ul>
        {families.map((f) => (
          <li key={f.ink} className={`ink-${f.ink}`}>
            <span className="ramp" aria-hidden="true"><i /><i /></span>
            <span><b>{f.name}</b> {f.steps}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
