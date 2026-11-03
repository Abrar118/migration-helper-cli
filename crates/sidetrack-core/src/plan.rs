//! Plans: every change sidetrack makes is first described as data (spec P0.8).
//! The CLI prints a plan, the git hook asks about it, the desktop app will draw it.

use serde::{Deserialize, Serialize};

use crate::engine::postgres::format_bytes;
use crate::registry::FileSet;
use crate::schema::SchemaDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Switch,
    Create,
    Snapshot,
    SnapshotRemove,
    Reset,
    Delete,
    Gc,
    Bench,
    Rename,
}

impl PlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanKind::Switch => "switch",
            PlanKind::Create => "create",
            PlanKind::Snapshot => "snapshot",
            PlanKind::SnapshotRemove => "snapshot-rm",
            PlanKind::Reset => "reset",
            PlanKind::Delete => "delete",
            PlanKind::Gc => "gc",
            PlanKind::Bench => "bench",
            PlanKind::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub kind: PlanKind,
    pub summary: String,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    /// How the target database's schema differs from the current one (switches
    /// between existing DB branches only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<SchemaDiff>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    /// Stop app services so nothing is connected while a database is copied.
    StopServices {
        services: Vec<String>,
    },
    TerminateConnections {
        db: String,
        sessions: Vec<String>,
    },
    CloneDatabase {
        source: String,
        target: String,
        size_bytes: u64,
    },
    CreateEmptyDatabase {
        name: String,
    },
    /// Disallow connections, so a snapshot can't drift.
    FreezeDatabase {
        name: String,
    },
    DropDatabase {
        name: String,
    },
    /// Drop a copy sidetrack made for itself in this same plan (e.g. `bench`).
    DropTemporaryDatabase {
        name: String,
    },
    RenameDatabase {
        from: String,
        to: String,
    },
    RewriteEnv {
        file: String,
        variable: String,
        from_db: String,
        to_db: String,
    },
    /// Proxy mode: send the app's connections for `database` to `to_db`.
    SetProxyTarget {
        database: String,
        from_db: String,
        to_db: String,
    },
    /// Recreate services so they pick up the env file.
    StartServices {
        services: Vec<String>,
    },
    RunMigrations {
        command: String,
        pending: Vec<String>,
    },
    RunSeed {
        command: String,
    },

    // Registry bookkeeping. Shown in JSON, hidden from the short text output.
    RegisterBranch {
        git_branch: String,
        db_name: String,
        parent_db: Option<String>,
        base_commit: Option<String>,
        migration_files: FileSet,
    },
    RecordMigrations {
        db_name: String,
        migration_files: FileSet,
    },
    TouchBranch {
        db_name: String,
    },
    RegisterSnapshot {
        branch_db: String,
        db_name: String,
        seq: i64,
        label: Option<String>,
        commit_id: Option<String>,
        migration_files: FileSet,
        size_bytes: u64,
    },
    ForgetBranch {
        db_name: String,
    },
    ForgetSnapshot {
        db_name: String,
    },
    /// Point a registry entry at a new database name and git branch (`rename`).
    RelinkBranch {
        db_name: String,
        new_db_name: String,
        new_git_branch: String,
    },
}

impl Step {
    /// Registry-only steps that change nothing outside sidetrack.
    pub fn is_bookkeeping(&self) -> bool {
        matches!(
            self,
            Step::RegisterBranch { .. }
                | Step::RecordMigrations { .. }
                | Step::TouchBranch { .. }
                | Step::RegisterSnapshot { .. }
                | Step::ForgetBranch { .. }
                | Step::ForgetSnapshot { .. }
                | Step::RelinkBranch { .. }
        )
    }

    /// Steps that lose data or kill sessions; these always need an explicit yes.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Step::DropDatabase { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Step::StopServices { services } => format!("Stop {}", services.join(", ")),
            Step::TerminateConnections { db, sessions } => format!(
                "Close {} connection{} to {db} ({})",
                sessions.len(),
                if sessions.len() == 1 { "" } else { "s" },
                summarize_sessions(sessions)
            ),
            Step::CloneDatabase {
                source,
                target,
                size_bytes,
            } => {
                format!(
                    "Create {target} as a copy of {source}  (+{})",
                    format_bytes(*size_bytes)
                )
            }
            Step::CreateEmptyDatabase { name } => format!("Create empty database {name}"),
            Step::FreezeDatabase { name } => format!("Freeze {name} (no connections allowed)"),
            Step::DropDatabase { name } => format!("DROP database {name}"),
            Step::DropTemporaryDatabase { name } => format!("Drop the temporary copy {name}"),
            Step::RenameDatabase { from, to } => format!("Rename {from} → {to}"),
            Step::RewriteEnv {
                file,
                variable,
                from_db,
                to_db,
            } => {
                format!("Point {variable} in {file} at {to_db} (was {from_db})")
            }
            Step::SetProxyTarget {
                database,
                from_db,
                to_db,
            } => format!("Route {database} through the proxy to {to_db} (was {from_db})"),
            Step::StartServices { services } => format!("Recreate {}", services.join(", ")),
            Step::RunMigrations { command, pending } => {
                let n = pending.len();
                let mut s = format!(
                    "Run {n} migration{}: `{command}`",
                    if n == 1 { "" } else { "s" }
                );
                for p in pending {
                    s.push_str(&format!("\n       · {}", short_file(p)));
                }
                s
            }
            Step::RunSeed { command } => format!("Seed: `{command}`"),
            Step::RegisterBranch {
                git_branch,
                db_name,
                ..
            } => {
                format!("Link git branch {git_branch} → {db_name}")
            }
            Step::RecordMigrations {
                db_name,
                migration_files,
            } => {
                format!("Record {} migrations for {db_name}", migration_files.len())
            }
            Step::TouchBranch { db_name } => format!("Mark {db_name} as used"),
            Step::RegisterSnapshot { db_name, .. } => format!("Record snapshot {db_name}"),
            Step::ForgetBranch { db_name } => format!("Forget {db_name}"),
            Step::ForgetSnapshot { db_name } => format!("Forget snapshot {db_name}"),
            Step::RelinkBranch {
                db_name,
                new_db_name,
                new_git_branch,
            } => format!("Link git branch {new_git_branch} → {new_db_name} (was {db_name})"),
        }
    }
}

fn summarize_sessions(sessions: &[String]) -> String {
    let mut uniq: Vec<&String> = sessions.iter().collect();
    uniq.sort();
    uniq.dedup();
    uniq.into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn short_file(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Plan {
    pub fn new(kind: PlanKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            schema: None,
            summary: summary.into(),
            steps: vec![],
            warnings: vec![],
            notes: vec![],
        }
    }

    pub fn push(&mut self, step: Step) {
        self.steps.push(step);
    }

    /// True when nothing outside the registry would change.
    pub fn is_noop(&self) -> bool {
        self.steps.iter().all(Step::is_bookkeeping)
    }

    pub fn has_destructive(&self) -> bool {
        self.steps.iter().any(Step::is_destructive)
    }

    pub fn added_bytes(&self) -> u64 {
        self.steps
            .iter()
            .map(|s| match s {
                Step::CloneDatabase { size_bytes, .. } => *size_bytes,
                _ => 0,
            })
            .sum()
    }

    /// Text for terminals. Bookkeeping steps are left out unless `verbose`.
    pub fn render(&self, verbose: bool) -> String {
        let mut out = format!("{}\n", self.summary);
        for w in &self.warnings {
            out.push_str(&format!("  ! {w}\n"));
        }
        for n in &self.notes {
            out.push_str(&format!("  · {n}\n"));
        }
        if let Some(diff) = &self.schema {
            out.push_str(&diff.render(verbose));
        }
        let shown: Vec<&Step> = self
            .steps
            .iter()
            .filter(|s| verbose || !s.is_bookkeeping())
            .collect();
        if shown.is_empty() {
            if self.notes.is_empty() {
                out.push_str("  Nothing to do.\n");
            }
            return out;
        }
        out.push_str("Plan:\n");
        for (i, s) in shown.iter().enumerate() {
            out.push_str(&format!("  {:>2}. {}\n", i + 1, s.describe()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_hides_bookkeeping() {
        let mut p = Plan::new(PlanKind::Switch, "Switch DB to feature_auth_db");
        p.push(Step::CloneDatabase {
            source: "dev_db".into(),
            target: "feature_auth_db".into(),
            size_bytes: 2 << 30,
        });
        p.push(Step::TouchBranch {
            db_name: "feature_auth_db".into(),
        });
        p.push(Step::RunMigrations {
            command: "make migrate".into(),
            pending: vec!["alembic/versions/3f9a_users.py".into()],
        });
        let text = p.render(false);
        assert!(
            text.contains("1. Create feature_auth_db as a copy of dev_db  (+2.0 GB)"),
            "{text}"
        );
        assert!(
            text.contains("2. Run 1 migration: `make migrate`\n       · 3f9a_users.py"),
            "{text}"
        );
        assert!(!text.contains("Mark"));
        assert!(p.render(true).contains("Mark feature_auth_db as used"));
        assert!(!p.is_noop());
        assert_eq!(p.added_bytes(), 2 << 30);
    }

    #[test]
    fn json_shape_is_tagged() {
        let s = Step::DropDatabase {
            name: "x_db".into(),
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"type":"drop_database","name":"x_db"}"#
        );
        assert!(s.is_destructive());
    }
}
