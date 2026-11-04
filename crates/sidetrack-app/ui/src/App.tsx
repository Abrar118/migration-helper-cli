import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { inApp, loadOverview } from "./api";
import { gb } from "./format";
import { Proof } from "./Proof";
import { ProjectList } from "./ProjectList";
import { ProjectPage } from "./ProjectPage";
import { EmptySheet, InSyncSheet, LoadErrorSheet, ProblemSheet } from "./Sheets";
import { createsDatabase, target } from "./steps";
import { applyTheme, loadTheme, type Theme } from "./theme";
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
  const busy = useRef(false);
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;

  const refresh = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setLoading(true);
    try {
      const o = await loadOverview();
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

  const back = useCallback(() => {
    setOpen(null);
    refresh();
  }, [refresh]);

  const projects = overview?.projects ?? [];
  const project = projects.find((p) => p.root === selected) ?? null;
  const disk = overview?.disk ?? null;

  const top = (
    <>
      <div className="top">
        <span className="mark">sidetrack</span>
        <nav className="nav" aria-label="Main">
          <button aria-current={view === "projects" ? "page" : undefined} onClick={() => setView("projects")}>Projects</button>
          <button aria-current={view === "settings" ? "page" : undefined} onClick={() => setView("settings")}>Settings</button>
        </nav>
      </div>
      {!inApp && <p className="demo">Demo data. Open the sidetrack app to see your projects.</p>}
    </>
  );

  if (open && view === "projects") {
    return (
      <div className="app">
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
    <div className="app">
      <aside className="col">
        {top}
        <h1>All projects</h1>
        {overview === null && !loadError ? (
          <p className="loading">Reading your projects…</p>
        ) : (
          <ProjectList projects={projects} selected={view === "projects" ? selected : null} onSelect={(r) => { setSelected(r); setView("projects"); }} />
        )}
        <div className="disk">
          {disk ? (
            <>
              <p className="n">{gb(disk.free_bytes)}<small>GB free</small></p>
              <p className="of">of {gb(disk.total_bytes)} GB on the Docker host</p>
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
          <Settings theme={theme} onTheme={setTheme} registry={overview?.registry ?? "…"} />
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
              <EmptySheet />
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
        project={p}
        plan={plan}
        disk={disk}
        onStart={() => props.onStart(plan)}
        onClose={() => props.onClose(pinned !== null)}
      />
    );
  }
  if (p.problem) return <ProblemSheet p={p} />;
  return <InSyncSheet p={p} />;
}

const Nw = ({ children }: { children: ReactNode }) => <span className="nw">{children}</span>;

/** The line above the sheet: what this project needs, in one sentence. */
function lead(p: ProjectView, applying: boolean): ReactNode {
  if (applying) return <>The plan for {p.name}</>;
  if (p.problem) return <>{p.name} needs attention</>;
  if (p.plan) {
    return createsDatabase(p.plan)
      ? <>{p.name} needs a database for <Nw>{p.git_branch}</Nw></>
      : <>{p.name} should switch to <Nw>{target(p.plan)}</Nw></>;
  }
  return <>{p.name} is on the right database</>;
}

/** Where the project is, how it routes, and (when a plan waits) what the app uses now. */
function where(p: ProjectView): string {
  const parts = [p.path];
  if (p.mode === "proxy") parts.push(`proxy ${p.listen?.replace(/^.*:/, ":") ?? ""}`);
  else if (p.mode) parts.push("env file");
  if (p.plan && p.active_db) parts.push(`the app is still on ${p.active_db}`);
  return parts.join(" · ");
}

function Settings({ theme, onTheme, registry }: { theme: Theme; onTheme: (t: Theme) => void; registry: string }) {
  const options: { value: Theme; label: string; hint: string }[] = [
    { value: "system", label: "Match the system", hint: "Light or dark, following your OS." },
    { value: "light", label: "Light", hint: "Paper on the light table." },
    { value: "dark", label: "Dark", hint: "The light table at night." },
  ];
  return (
    <section className="settings">
      <h2>Settings</h2>
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
      <div className="setting">
        <h3>Registry</h3>
        <p className="db">{registry}</p>
        <p className="note">One file for every project on this machine. Set SIDETRACK_REGISTRY to use another.</p>
      </div>
    </section>
  );
}
