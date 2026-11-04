//! What the frontend shows, gathered from the core. Plans are passed through
//! unchanged (their JSON format is the stable contract); the rest is a summary
//! of `World` per project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
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
    // Like the CLI: start the proxy before planning, so the plan holds
    // connections instead of stopping the app.
    let start_failed = ws.proxy_needs_start().then(|| match sidetrack_exe() {
        Some(exe) => ws
            .start_proxy(&exe, None)
            .err()
            .map(|e| format!("couldn't start the proxy: {e:#}")),
        None => Some(
            "the proxy isn't running, and the `sidetrack` CLI isn't on PATH to start it; \
             run `sidetrack proxy --daemon` in the project"
                .to_string(),
        ),
    });
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
    view.warnings.extend(start_failed.flatten());
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
    /// The database service, then the app services sidetrack restarts.
    pub services: Vec<ServiceView>,
    /// Registry and server disagree (never fixed automatically).
    pub warnings: Vec<String>,
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
    /// `None` when Docker couldn't be asked.
    pub running: Option<bool>,
}

pub fn project_detail(root: &str) -> Result<ProjectDetail> {
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
    let db_service = &ws.cfg.database.service;
    let mut names = vec![db_service.clone()];
    names.extend(
        ws.cfg
            .routing
            .restart
            .iter()
            .filter(|s| *s != db_service)
            .cloned(),
    );
    let services = names
        .into_iter()
        .map(|name| ServiceView {
            running: running.as_ref().map(|r| r.contains(&name)),
            database: &name == db_service,
            name,
        })
        .collect();

    Ok(ProjectDetail {
        warnings: world
            .as_ref()
            .map(|w| ws.planner(w).reconcile())
            .unwrap_or_default(),
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

/// The sidetrack CLI, which runs the proxy: next to this app, then on PATH,
/// then in `~/.cargo/bin` (apps opened from a desktop often get a shorter PATH
/// than the terminal).
fn sidetrack_exe() -> Option<PathBuf> {
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
