//! What the frontend shows, gathered from the core. Plans are passed through
//! unchanged (their JSON format is the stable contract); the rest is a summary
//! of `World` per project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use sidetrack_core::compose::ConfigView;
use sidetrack_core::git::Commit;
use sidetrack_core::plan::Plan;
use sidetrack_core::planner::{DiskSpace, MigrationStatus, World};
use sidetrack_core::registry::{self, Registry};
use sidetrack_core::workspace::Workspace;

#[derive(Debug, Serialize)]
pub struct Overview {
    pub projects: Vec<ProjectView>,
    /// Disk where the database containers keep their data (first project that reports it).
    pub disk: Option<DiskSpace>,
    pub registry: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectView {
    /// The main worktree's root: the registry key, and what `plan_op` and `apply_op` take.
    pub root: String,
    /// `root` for display, with the home directory as `~`.
    pub path: String,
    pub name: String,
    /// "env" or "proxy".
    pub mode: String,
    /// Proxy mode: where the proxy listens.
    pub listen: Option<String>,
    /// Proxy mode: whether it's accepting connections.
    pub proxy_running: Option<bool>,
    pub git_branch: Option<String>,
    pub active_db: Option<String>,
    /// Why the project's state couldn't be read (Docker down, database stopped, …).
    /// When set, only the fields above it are filled.
    pub problem: Option<String>,
    pub branches: usize,
    pub snapshots: usize,
    pub running_services: Vec<String>,
    /// Databases other git worktrees use: (database, worktree path).
    pub elsewhere: Vec<(String, String)>,
    pub migrations: Option<MigrationStatus>,
    /// Registry and server disagree (never fixed automatically).
    pub warnings: Vec<String>,
    /// The switch plan for the checked-out branch, when it would change something.
    pub plan: Option<Plan>,
}

pub fn overview() -> Result<Overview> {
    let reg_path = registry::default_path()?;
    let projects = Registry::open(&reg_path)?.projects()?;
    // Each project runs git, docker and psql; read them side by side.
    let views: Vec<(ProjectView, Option<DiskSpace>)> = std::thread::scope(|s| {
        let handles: Vec<_> = projects
            .iter()
            .map(|p| s.spawn(|| project_view(&p.repo_root)))
            .collect();
        handles
            .into_iter()
            .zip(&projects)
            .map(|(h, p)| {
                h.join()
                    .unwrap_or_else(|_| (unreadable(&p.repo_root, "reading it crashed"), None))
            })
            .collect()
    });
    let disk = views.iter().find_map(|(_, d)| *d);
    Ok(Overview {
        projects: views.into_iter().map(|(v, _)| v).collect(),
        disk,
        registry: reg_path.display().to_string(),
    })
}

fn project_view(root: &str) -> (ProjectView, Option<DiskSpace>) {
    if let Some(problem) = folder_gone(root) {
        return (unreadable(root, &problem), None);
    }
    let ws = match Workspace::open(Path::new(root)) {
        Ok(ws) => ws,
        Err(e) => return (unreadable(root, &format!("{e:#}")), None),
    };
    // Like the CLI: start the proxy before planning, so the plan holds
    // connections instead of stopping the app.
    let start_failed = autostart(&ws);
    let r = &ws.cfg.routing;
    let mut view = ProjectView {
        mode: r.mode.clone(),
        listen: r.is_proxy().then(|| ws.proxy_listen()),
        git_branch: ws.git.current_branch().ok().flatten(),
        active_db: ws.active_db().ok().flatten(),
        ..unreadable(root, "")
    };
    view.problem = None;
    let world = match ws.world() {
        Ok(w) => w,
        Err(e) => {
            view.problem = Some(format!("{e:#}"));
            return (view, None);
        }
    };
    let planner = ws.planner(&world);
    view.proxy_running = world.proxy_running;
    view.branches = world.branches.len();
    view.snapshots = world.snapshots.len();
    view.running_services = world.running_services.clone();
    view.elsewhere = world
        .elsewhere
        .iter()
        .map(|(db, wt)| (db.clone(), tilde(wt)))
        .collect();
    view.warnings = planner.reconcile();
    view.warnings.extend(start_failed);
    if let Some(b) = &view.git_branch {
        view.migrations = planner.migration_status(b).ok().flatten();
    }
    match switch_plan(&ws, &world) {
        Ok(Some(plan)) if !plan.is_noop() => view.plan = Some(plan),
        Ok(_) => {}
        Err(e) => view.problem = Some(format!("{e:#}")),
    }
    (view, world.disk)
}

/// How many commits the project view shows per git branch.
const COMMITS_PER_BRANCH: usize = 5;

/// One project's git branches and DB branches, side by side.
#[derive(Debug, Serialize)]
pub struct ProjectDetail {
    pub root: String,
    pub path: String,
    pub name: String,
    pub mode: String,
    /// The branch checked out in the main worktree.
    pub git_branch: Option<String>,
    pub active_db: Option<String>,
    /// Why the server couldn't be read. The registry and git parts are still
    /// filled, and `on_server` / `size_bytes` are unknown.
    pub problem: Option<String>,
    /// Checked out here first, then the most recently committed to.
    pub git_branches: Vec<GitBranchView>,
    /// Oldest first; `parent` links them into a tree.
    pub db_branches: Vec<DbBranchView>,
    /// The database service, the app services sidetrack restarts, then the
    /// rest of the compose project's services.
    pub services: Vec<ServiceView>,
    /// How the project is wired: compose, routing, migrations (from `sidetrack.toml`).
    pub structure: StructureView,
    /// Registry and server disagree (never fixed automatically).
    pub warnings: Vec<String>,
    /// Disk where the database container keeps its data.
    pub disk: Option<DiskSpace>,
}

#[derive(Debug, Serialize)]
pub struct GitBranchView {
    pub name: String,
    /// Its DB branch, if it has one.
    pub db: Option<String>,
    /// Checked out in the main worktree.
    pub here: bool,
    /// Checked out in another worktree: that worktree's path.
    pub worktree: Option<String>,
    pub commits: Vec<Commit>,
}

#[derive(Debug, Serialize)]
pub struct DbBranchView {
    pub db_name: String,
    pub git_branch: String,
    /// The DB branch it was copied from.
    pub parent: Option<String>,
    pub git_branch_exists: bool,
    /// `None` when the server couldn't be read.
    pub on_server: Option<bool>,
    pub size_bytes: Option<u64>,
    pub adopted: bool,
    pub created_at: i64,
    pub last_used_at: i64,
    /// The app in the main worktree uses it.
    pub active: bool,
    /// Another worktree's app uses it: that worktree's path.
    pub worktree: Option<String>,
    /// Oldest first.
    pub snapshots: Vec<SnapshotView>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotView {
    pub db_name: String,
    pub seq: i64,
    pub label: Option<String>,
    pub size_bytes: Option<u64>,
    pub created_at: i64,
    pub on_server: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ServiceView {
    pub name: String,
    pub database: bool,
    /// Restarted (or re-pointed) when the database changes: it reads the DB name.
    pub routed: bool,
    /// `None` when Docker couldn't be asked.
    pub running: Option<bool>,
    /// From `docker compose config`; `None` when it couldn't be read or isn't set.
    pub image: Option<String>,
    /// The build context, for a service built from the repo.
    pub build: Option<String>,
    pub depends_on: Vec<String>,
    pub volumes: Vec<VolumeView>,
}

#[derive(Debug, Serialize)]
pub struct VolumeView {
    /// `volume`, `bind`, `tmpfs`, …
    pub kind: Option<String>,
    /// The named volume or host path.
    pub source: Option<String>,
    /// The path inside the container.
    pub target: Option<String>,
}

/// The parts of `sidetrack.toml` that say how the project is put together.
#[derive(Debug, Serialize)]
pub struct StructureView {
    /// The compose project's name, when compose could be read.
    pub compose_project: Option<String>,
    /// Extra `-f` files; empty means compose's defaults.
    pub compose_files: Vec<String>,
    pub engine: String,
    pub db_service: String,
    pub db_user: String,
    pub routing_mode: String,
    pub routing_file: String,
    pub routing_variable: String,
    pub listen: Option<String>,
    pub restart: Vec<String>,
    pub migrations_tool: String,
    pub migrations_dirs: Vec<String>,
    pub migrations_command: String,
    pub seed: Option<String>,
    pub main_branch: Option<String>,
}

pub fn project_detail(root: &str) -> Result<ProjectDetail> {
    if let Some(problem) = folder_gone(root) {
        anyhow::bail!(problem);
    }
    let ws = Workspace::open(Path::new(root))?;
    let project = ws.project()?;
    let git_branch = ws.git.current_branch()?;
    let active_db = ws.active_db().ok().flatten();
    // Without Docker or the database, the registry and git still say a lot.
    let (world, problem) = match ws.world() {
        Ok(w) => (Some(w), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let (branches, snapshots, elsewhere) = match &world {
        Some(w) => (w.branches.clone(), w.snapshots.clone(), w.elsewhere.clone()),
        None => (
            ws.reg.branches(project.id)?,
            ws.reg.snapshots(project.id)?,
            ws.active_elsewhere(),
        ),
    };
    let server = world.as_ref().map(|w| &w.server_dbs);
    let on_server = |db: &str| server.map(|s| s.contains_key(db));
    let size = |db: &str| server.and_then(|s| s.get(db)).map(|d| d.size_bytes);

    let local = ws.git.local_branches()?;
    // (branch → path) for the other worktrees; the first entry is this one.
    let checked_out: BTreeMap<String, String> = ws
        .git
        .worktrees()
        .unwrap_or_default()
        .into_iter()
        .skip(1)
        .filter_map(|(path, b)| Some((b?, tilde(&path.to_string_lossy()))))
        .collect();
    let mut git_branches: Vec<GitBranchView> = local
        .iter()
        .map(|name| GitBranchView {
            name: name.clone(),
            db: branches
                .iter()
                .find(|b| &b.git_branch == name)
                .map(|b| b.db_name.clone()),
            here: git_branch.as_ref() == Some(name),
            worktree: checked_out.get(name).cloned(),
            commits: ws
                .git
                .log(&format!("refs/heads/{name}"), COMMITS_PER_BRANCH)
                .unwrap_or_default(),
        })
        .collect();
    git_branches.sort_by_key(|b| {
        let newest = b.commits.first().map_or(0, |c| c.time);
        (!b.here, std::cmp::Reverse(newest))
    });

    let mut db_branches: Vec<DbBranchView> = branches
        .iter()
        .map(|b| DbBranchView {
            db_name: b.db_name.clone(),
            git_branch: b.git_branch.clone(),
            parent: b
                .parent_id
                .and_then(|id| branches.iter().find(|p| p.id == id))
                .map(|p| p.db_name.clone()),
            git_branch_exists: local.contains(&b.git_branch),
            on_server: on_server(&b.db_name),
            size_bytes: size(&b.db_name),
            adopted: b.adopted,
            created_at: b.created_at,
            last_used_at: b.last_used_at,
            active: active_db.as_deref() == Some(b.db_name.as_str()),
            worktree: elsewhere.get(&b.db_name).map(|p| tilde(p)),
            snapshots: snapshots
                .iter()
                .filter(|s| s.branch_id == b.id)
                .map(|s| SnapshotView {
                    db_name: s.db_name.clone(),
                    seq: s.seq,
                    label: s.label.clone(),
                    size_bytes: s.size_bytes.or_else(|| size(&s.db_name)),
                    created_at: s.created_at,
                    on_server: on_server(&s.db_name),
                })
                .collect(),
        })
        .collect();
    db_branches.sort_by_key(|b| b.created_at);

    // `root` is the main worktree, so its compose runs the database too.
    let running = match &world {
        Some(w) => Some(w.running_services.clone()),
        None => ws.compose.running_services().ok(),
    };
    let cfg = &ws.cfg;
    let db_service = &cfg.database.service;
    let model = ws.compose.config(ConfigView::Resolved).ok();
    let mut names = vec![db_service.clone()];
    names.extend(
        cfg.routing
            .restart
            .iter()
            .filter(|s| *s != db_service)
            .cloned(),
    );
    if let Some(m) = &model {
        names.extend(
            m.services
                .keys()
                .filter(|s| !names.contains(s))
                .cloned()
                .collect::<Vec<_>>(),
        );
    }
    let services = names
        .into_iter()
        .map(|name| {
            let svc = model.as_ref().and_then(|m| m.services.get(&name));
            ServiceView {
                running: running.as_ref().map(|r| r.contains(&name)),
                database: &name == db_service,
                routed: cfg.routing.restart.contains(&name),
                image: svc.and_then(|s| s.image.clone()),
                build: svc.and_then(|s| s.build.as_ref()?.context.as_deref().map(tilde)),
                depends_on: svc.map(|s| s.depends_on()).unwrap_or_default(),
                volumes: svc
                    .map(|s| {
                        s.volumes
                            .iter()
                            .map(|v| VolumeView {
                                kind: v.kind.clone(),
                                source: v.source.as_deref().map(tilde),
                                target: v.target.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                name,
            }
        })
        .collect();
    let structure = StructureView {
        compose_project: model
            .as_ref()
            .map(|m| m.name.clone())
            .filter(|n| !n.is_empty()),
        compose_files: cfg.compose.files.clone(),
        engine: cfg.database.engine.clone(),
        db_service: db_service.clone(),
        db_user: cfg.database.user.clone(),
        routing_mode: cfg.routing.mode.clone(),
        routing_file: cfg.routing.file.clone(),
        routing_variable: cfg.routing.variable.clone(),
        listen: cfg.routing.listen.clone(),
        restart: cfg.routing.restart.clone(),
        migrations_tool: cfg.migrations.tool.clone(),
        migrations_dirs: cfg.migrations.dirs.clone(),
        migrations_command: cfg.migrations.command.clone(),
        seed: cfg.migrations.seed.clone(),
        main_branch: cfg.git.main_branch.clone(),
    };

    Ok(ProjectDetail {
        warnings: world
            .as_ref()
            .map(|w| ws.planner(w).reconcile())
            .unwrap_or_default(),
        disk: world.as_ref().and_then(|w| w.disk),
        root: root.to_string(),
        path: tilde(root),
        name: unreadable(root, "").name,
        mode: ws.cfg.routing.mode.clone(),
        git_branch,
        active_db,
        problem,
        git_branches,
        db_branches,
        services,
        structure,
    })
}

/// The plan `sidetrack switch` would show for the checked-out branch, with its
/// schema summary. `None` on a detached HEAD.
pub fn switch_plan(ws: &Workspace, world: &World) -> Result<Option<Plan>> {
    let Some(branch) = ws.git.current_branch()? else {
        return Ok(None);
    };
    let mut plan = ws.planner(world).switch(&branch)?;
    ws.annotate_schema(&mut plan);
    Ok(Some(plan))
}

/// Proxy mode with autostart on: start the proxy if it isn't running.
/// Returns why it couldn't be started; never fails.
pub fn autostart(ws: &Workspace) -> Option<String> {
    if !ws.proxy_needs_start() {
        return None;
    }
    match sidetrack_exe() {
        Some(exe) => ws
            .start_proxy(&exe, None)
            .err()
            .map(|e| format!("couldn't start the proxy: {e:#}")),
        None => Some(
            "the proxy isn't running, and the `sidetrack` CLI isn't on PATH to start it; \
             run `sidetrack proxy --daemon` in the project"
                .to_string(),
        ),
    }
}

/// The sidetrack CLI, which runs the proxy: next to this app, then on PATH,
/// then in `~/.cargo/bin` (apps opened from a desktop often get a shorter PATH
/// than the terminal).
pub fn sidetrack_exe() -> Option<PathBuf> {
    let name = format!("sidetrack{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    let cargo = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".cargo").join("bin"));
    beside
        .into_iter()
        .chain(path)
        .chain(cargo)
        .map(|dir| dir.join(&name))
        .find(|f| f.is_file())
}

/// `/home/me/code/shop` → `~/code/shop`.
fn tilde(root: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
    match home {
        Ok(h) if !h.is_empty() && root.starts_with(&h) => format!("~{}", &root[h.len()..]),
        _ => root.to_string(),
    }
}

/// A registered project whose folder was moved or deleted. Said plainly, rather
/// than as git's "cannot change to …" error.
fn folder_gone(root: &str) -> Option<String> {
    (!Path::new(root).is_dir()).then(|| {
        format!(
            "its folder is gone; it was at {}. If it moved, add it again from where it is now (Add project)",
            tilde(root)
        )
    })
}

fn unreadable(root: &str, problem: &str) -> ProjectView {
    ProjectView {
        root: root.to_string(),
        path: tilde(root),
        name: Path::new(root)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string()),
        mode: String::new(),
        listen: None,
        proxy_running: None,
        git_branch: None,
        active_db: None,
        problem: Some(problem.to_string()),
        branches: 0,
        snapshots: 0,
        running_services: vec![],
        elsewhere: vec![],
        migrations: None,
        warnings: vec![],
        plan: None,
    }
}
