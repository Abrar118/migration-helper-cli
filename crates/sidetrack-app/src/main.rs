//! sidetrack desktop app: the Tauri shell around `sidetrack-core`.
//!
//! It drives the core the same way the CLI does (`Workspace::world`, `Planner`,
//! `Applier`) and never edits anything itself. The frontend lives in `ui/`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ops;
mod views;

use anyhow::{bail, Result};
use sidetrack_core::apply::Report;
use sidetrack_core::notify;
use sidetrack_core::plan::Plan;
use sidetrack_core::registry::{self, Registry};
use sidetrack_core::workspace::Workspace;
use tauri::{AppHandle, Emitter, Manager, UserAttentionType};

use crate::ops::{Op, Planned};
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

/// The plan for `op` on the project at `root`. Like the CLI, it starts the
/// proxy first (proxy mode with autostart), so copies hold connections instead
/// of stopping the app. If the proxy won't start, the reason comes back with
/// the plan, to show as a Query.
#[tauri::command]
async fn plan_op(root: String, op: Op) -> Result<Planned, String> {
    blocking(move || {
        let ws = Workspace::open(root.as_ref())?;
        let warnings = views::autostart(&ws).into_iter().collect();
        let world = ws.world()?;
        let plan = ops::plan(&ws, &world, &op)?;
        Ok(Planned { plan, warnings })
    })
    .await
}

/// Apply the plan for `op` that the user was shown for the project at `root`.
///
/// The plan is built again first. If it no longer matches `shown`, nothing runs:
/// the user only ever applies a plan they have seen. Destructive plans also need
/// `confirm_destructive`.
#[tauri::command]
async fn apply_op(
    app: AppHandle,
    root: String,
    op: Op,
    shown: Plan,
    confirm_destructive: bool,
) -> Result<Report, String> {
    blocking(move || {
        let ws = Workspace::open(root.as_ref())?;
        let world = ws.world()?;
        let plan = ops::plan(&ws, &world, &op)?;
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

/// A git checkout without a terminal (a GUI client) left a project needing a
/// switch, and the hook asked us to show it. Only registered projects count.
/// The frontend reads the project again and shows its switch proof; nothing is
/// applied until the user approves it there.
fn on_checkout(app: &AppHandle, root: &str) -> bool {
    let known = registry::default_path()
        .and_then(|p| Registry::open(&p))
        .and_then(|r| r.project_by_root(root))
        .is_ok_and(|p| p.is_some());
    if known {
        let _ = app.emit("checkout", root);
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.request_user_attention(Some(UserAttentionType::Informational));
        }
    }
    known
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let listening = notify::port_file()
                .and_then(|f| notify::listen(&f, move |root| on_checkout(&handle, root)));
            if let Err(e) = listening {
                eprintln!("sidetrack: the git hook can't reach the app: {e:#}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            overview,
            project_detail,
            plan_op,
            apply_op
        ])
        .run(tauri::generate_context!())
        .expect("error while running the sidetrack app");
}
