//! The git `post-checkout` hook (spec P0.10).
//!
//! Rules: the hook never fails or blocks a checkout, never prompts without a
//! terminal, and stays quiet when there's nothing to do.

use std::path::Path;

use anyhow::Result;
use sidetrack_core::config::Config;
use sidetrack_core::git::Git;
use sidetrack_core::notify;
use sidetrack_core::workspace::Workspace;

use crate::ui::{self, Opts};

/// Install the hook. It falls back on this binary when `sidetrack` isn't on PATH.
pub fn install(git: &Git) -> Result<String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "sidetrack".into());
    sidetrack_core::hook::install(git, &exe)
}

pub use sidetrack_core::hook::{is_installed, uninstall};

/// Entry point for the hook. Never returns an error: problems are printed.
pub fn run(dir: &Path, _old: &str, _new: &str, flag: &str) {
    if let Err(e) = run_inner(dir, flag) {
        eprintln!("sidetrack: {e:#}");
    }
}

fn run_inner(dir: &Path, flag: &str) -> Result<()> {
    // Note: we don't skip when old == new. `git checkout -b feature` keeps the same
    // commit but still needs its own database.
    if std::env::var("SIDETRACK_SKIP").is_ok_and(|v| !v.is_empty() && v != "0") || flag != "1" {
        return Ok(());
    }
    let Ok(git) = Git::discover(dir) else {
        return Ok(());
    };
    if !Config::exists(git.root()) || git.operation_in_progress()?.is_some() {
        return Ok(());
    }
    let ws = Workspace::open(git.root())?;
    let Some(branch) = git.current_branch()? else {
        let active = ws.active_db()?.unwrap_or_else(|| "?".into());
        eprintln!("sidetrack: detached HEAD; database left on {active}");
        return Ok(());
    };
    let Ok(project) = ws.project() else {
        eprintln!("sidetrack: run `sidetrack init` to link databases to branches in this repo");
        return Ok(());
    };
    crate::autostart_proxy(&ws);
    let world = match ws.world() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("sidetrack: {e:#}; database left as is");
            return Ok(());
        }
    };
    let plan = ws.planner(&world).switch(&branch)?;
    if plan.is_noop() {
        // Already on the right database: just record it, silently.
        ws.applier(&project).apply(&plan, &mut |_| {})?;
        return Ok(());
    }
    let auto = project.auto_approve && !plan.has_destructive();
    if !auto && !ui::has_tty() {
        // A GUI client: hand it to the desktop app if it's open. The app only
        // plans for each project's main worktree.
        let in_app = !ws.is_linked_worktree() && notify::send(&ws.root_key());
        let next = if in_app {
            "approve it in the sidetrack app, or run `sidetrack switch`"
        } else {
            "run `sidetrack switch` to apply"
        };
        eprintln!("sidetrack: {} — {next}", plan.summary);
        return Ok(());
    }
    let mut plan = plan;
    ws.annotate_schema(&mut plan);
    println!();
    let ok = ui::run_plan(&ws, &project, &plan, &Opts::default())?;
    if !ok {
        eprintln!("sidetrack: the git checkout itself is fine; only the database step failed");
    }
    Ok(())
}
