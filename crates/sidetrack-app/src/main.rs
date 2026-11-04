//! sidetrack desktop app: the Tauri shell around `sidetrack-core`.
//!
//! It drives the core the same way the CLI does (`Workspace::world`, `Planner`,
//! `Applier`) and never edits anything itself. The frontend lives in `ui/`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod views;

use anyhow::{bail, Context, Result};
use sidetrack_core::apply::Report;
use sidetrack_core::plan::Plan;
use sidetrack_core::workspace::Workspace;
use tauri::{AppHandle, Emitter};

use crate::views::{Overview, ProjectDetail};

/// Every registered project with its current state and, when it's out of sync,
/// the switch plan for its checked-out branch.
#[tauri::command]
async fn overview() -> Result<Overview, String> {
    blocking(views::overview).await
}

/// One project's git branches, DB branch tree, snapshots, worktrees,
/// containers and recent commits. Read-only.
#[tauri::command]
async fn project_detail(root: String) -> Result<ProjectDetail, String> {
    blocking(move || views::project_detail(&root)).await
}

/// Apply the switch plan the user was shown for the project at `root`.
///
/// The plan is built again first. If it no longer matches `shown`, nothing runs:
/// the user only ever applies a plan they have seen. Destructive plans also need
/// `confirm_destructive`.
#[tauri::command]
async fn apply_switch(
    app: AppHandle,
    root: String,
    shown: Plan,
    confirm_destructive: bool,
) -> Result<Report, String> {
    blocking(move || {
        let ws = Workspace::open(root.as_ref())?;
        let world = ws.world()?;
        let plan = views::switch_plan(&ws, &world)?
            .context("HEAD is detached, so there's no branch to switch to")?;
        if plan != shown {
            bail!("the plan changed since it was shown (something else changed the project); review the new plan and apply again");
        }
        if plan.has_destructive() && !confirm_destructive {
            bail!("this plan deletes data; confirm it before applying");
        }
        let project = world.project.clone();
        ws.applier(&project).apply(&plan, &mut |line| {
            let _ = app.emit("apply-progress", line);
        })
    })
    .await
}

/// Run blocking core work (git, docker, psql) off the main thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            overview,
            project_detail,
            apply_switch
        ])
        .run(tauri::generate_context!())
        .expect("error while running the sidetrack app");
}
