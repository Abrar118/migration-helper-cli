//! What the frontend shows, gathered from the core. Plans are passed through
//! unchanged (their JSON format is the stable contract); the rest is a summary
//! of `World` per project.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
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
    /// The main worktree's root: the registry key, and what `apply_switch` takes.
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
    let ws = match Workspace::open(Path::new(root)) {
        Ok(ws) => ws,
        Err(e) => return (unreadable(root, &format!("{e:#}")), None),
    };
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

/// `/home/me/code/shop` → `~/code/shop`.
fn tilde(root: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
    match home {
        Ok(h) if !h.is_empty() && root.starts_with(&h) => format!("~{}", &root[h.len()..]),
        _ => root.to_string(),
    }
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
