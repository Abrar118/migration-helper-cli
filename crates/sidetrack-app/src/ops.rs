//! The operations the app plans and applies. Each one calls the planner the
//! way the matching CLI command does (`plan_command` in `crates/sidetrack`).

use anyhow::{Context, Result};
use serde::Deserialize;
use sidetrack_core::plan::Plan;
use sidetrack_core::planner::{ResetTo, World};
use sidetrack_core::workspace::Workspace;

use crate::views;

/// One operation, as the frontend sends it: `{"op": "delete", "name": "feature_x_db"}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Switch to the checked-out branch (`sidetrack switch`).
    Switch,
    /// `sidetrack create <git_branch> [--from <from>] [--empty]`.
    Create {
        git_branch: String,
        from: Option<String>,
        #[serde(default)]
        empty: bool,
    },
    /// Snapshot the active database (`sidetrack snapshot [label]`).
    Snapshot { label: Option<String> },
    /// Reset the active database (`sidetrack reset [--to <to>] [--parent]`).
    Reset {
        to: Option<String>,
        #[serde(default)]
        parent: bool,
    },
    /// `sidetrack delete <name>`: a git branch or a database name.
    Delete { name: String },
    /// `sidetrack rename <old> <new>`.
    Rename { old: String, new: String },
    /// `sidetrack gc`.
    Gc,
}

/// The plan the CLI would show for `op`.
pub fn plan(ws: &Workspace, world: &World, op: &Op) -> Result<Plan> {
    let planner = ws.planner(world);
    match op {
        Op::Switch => views::switch_plan(ws, world)?
            .context("HEAD is detached, so there's no branch to switch to"),
        Op::Create {
            git_branch,
            from,
            empty,
        } => planner.create(git_branch, from.as_deref(), *empty),
        Op::Snapshot { label } => {
            let active = world
                .active_db
                .as_deref()
                .context("can't read the active database")?;
            let branch = planner
                .branch_for_db(active)
                .with_context(|| format!("{active} isn't linked to a git branch"))?;
            let seq = ws.reg.next_snapshot_seq(branch.id)?;
            planner.snapshot(label.as_deref(), seq)
        }
        Op::Reset { to, parent } => planner.reset(&match (to, parent) {
            (Some(n), _) => ResetTo::Snapshot(n.clone()),
            (None, true) => ResetTo::Parent,
            (None, false) => ResetTo::Latest,
        }),
        Op::Delete { name } => planner.delete(name),
        Op::Rename { old, new } => planner.rename(old, new),
        Op::Gc => planner.gc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(json: &str) -> Op {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn reads_the_shapes_the_frontend_sends() {
        assert_eq!(op(r#"{"op":"switch"}"#), Op::Switch);
        assert_eq!(op(r#"{"op":"gc"}"#), Op::Gc);
        assert_eq!(
            op(r#"{"op":"create","git_branch":"fix/x","from":null,"empty":true}"#),
            Op::Create {
                git_branch: "fix/x".into(),
                from: None,
                empty: true
            }
        );
        assert_eq!(
            op(r#"{"op":"snapshot","label":"before"}"#),
            Op::Snapshot {
                label: Some("before".into())
            }
        );
        assert_eq!(
            op(r#"{"op":"reset","to":null,"parent":true}"#),
            Op::Reset {
                to: None,
                parent: true
            }
        );
        assert_eq!(
            op(r#"{"op":"rename","old":"a_db","new":"feature/b"}"#),
            Op::Rename {
                old: "a_db".into(),
                new: "feature/b".into()
            }
        );
        assert_eq!(
            op(r#"{"op":"delete","name":"a_db"}"#),
            Op::Delete {
                name: "a_db".into()
            }
        );
    }
}
