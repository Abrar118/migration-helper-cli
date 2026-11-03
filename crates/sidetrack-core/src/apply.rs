//! Runs a plan. This is the only code that changes anything (spec P0.8).
//!
//! If a step fails, completed steps are undone in reverse order. The exception is
//! the user's own migration or seed command: if that fails, the new database stays
//! selected so the failure can be debugged.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::compose::Compose;
use crate::config::Config;
use crate::engine::postgres::Postgres;
use crate::envfile;
use crate::plan::{Plan, Step};
use crate::process;
use crate::registry::{FileSet, NewBranch, NewSnapshot, Project, Registry};

pub struct Applier<'a> {
    pub cfg: &'a Config,
    pub root: &'a Path,
    pub compose: &'a Compose,
    pub pg: &'a Postgres,
    pub reg: &'a Registry,
    pub project: &'a Project,
    /// File the proxy reads the active DB branch from (proxy mode).
    pub proxy_target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Done,
    /// A step failed; everything before it was undone.
    RolledBack {
        step: String,
        error: String,
        undo_errors: Vec<String>,
    },
    /// A step failed and could not be (fully) undone; `note` says what state things are in.
    Stopped {
        step: String,
        error: String,
        note: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub outcome: Outcome,
    pub log: Vec<String>,
    /// Seconds each completed step took, in plan order.
    pub timings: Vec<StepTiming>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepTiming {
    /// Index into `plan.steps`.
    pub step: usize,
    pub seconds: f64,
    /// Extra detail, e.g. which copy method was used.
    pub detail: Option<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.outcome == Outcome::Done
    }
}

enum Undo {
    DropDb(String),
    RestoreEnv {
        path: PathBuf,
        content: String,
    },
    RenameBack {
        from: String,
        to: String,
    },
    StartServices(Vec<String>),
    DeleteBranchRow(i64),
    DeleteSnapshotRow(i64),
    RestoreFiles {
        branch_id: i64,
        files: FileSet,
    },
    Relink {
        id: i64,
        db_name: String,
        git_branch: String,
    },
    /// Can't be undone; undo stops here.
    Barrier(String),
    Nothing,
}

impl Applier<'_> {
    pub fn apply(&self, plan: &Plan, progress: &mut dyn FnMut(&str)) -> Result<Report> {
        let plan_json = serde_json::to_string(plan)?;
        let op = self
            .reg
            .start_operation(self.project.id, plan.kind.as_str(), &plan_json)?;
        let mut log = vec![];
        let mut say = |msg: String, log: &mut Vec<String>| {
            progress(&msg);
            log.push(msg);
        };

        let mut undo: Vec<Undo> = vec![];
        let mut timings = vec![];
        for (index, step) in plan.steps.iter().enumerate() {
            if !step.is_bookkeeping() {
                say(format!("→ {}", first_line(&step.describe())), &mut log);
            }
            let started = Instant::now();
            match self.run(step) {
                Ok((u, detail)) => {
                    let seconds = started.elapsed().as_secs_f64();
                    if let Some(d) = &detail {
                        say(format!("  {d} ({seconds:.1}s)"), &mut log);
                    }
                    timings.push(StepTiming {
                        step: index,
                        seconds,
                        detail,
                    });
                    undo.push(u);
                }
                Err(e) => {
                    let error = format!("{e:#}");
                    let step_desc = first_line(&step.describe()).to_string();
                    say(format!("  ✗ {error}"), &mut log);
                    let outcome = if matches!(
                        step,
                        Step::RunMigrations { .. } | Step::RunSeed { .. }
                    ) {
                        Outcome::Stopped {
                            step: step_desc,
                            error,
                            note: "the database switch itself finished; your migration command failed, \
                                   so the new database stays selected. Fix it and re-run your migrations \
                                   (or `sidetrack switch` to retry)."
                                .into(),
                        }
                    } else {
                        self.roll_back(undo, step_desc, error, &mut |m| {
                            say(m.to_string(), &mut log)
                        })
                    };
                    let status = match &outcome {
                        Outcome::Done => "ok",
                        Outcome::RolledBack { .. } => "rolled_back",
                        Outcome::Stopped { .. } => "stopped",
                    };
                    self.reg
                        .finish_operation(op, status, Some(&format!("{outcome:?}")))?;
                    return Ok(Report {
                        outcome,
                        log,
                        timings,
                    });
                }
            }
        }
        self.reg.finish_operation(op, "ok", None)?;
        Ok(Report {
            outcome: Outcome::Done,
            log,
            timings,
        })
    }

    fn roll_back(
        &self,
        undo: Vec<Undo>,
        step: String,
        error: String,
        say: &mut dyn FnMut(&str),
    ) -> Outcome {
        let mut undo_errors = vec![];
        let mut restart: Option<Vec<String>> = None;
        let mut env_restored = false;
        for u in undo.into_iter().rev() {
            let r = match u {
                Undo::Barrier(what) => {
                    return Outcome::Stopped {
                        step,
                        error,
                        note: format!(
                            "could not roll back past an earlier step that can't be undone: {what}"
                        ),
                    };
                }
                Undo::Nothing => Ok(()),
                Undo::DropDb(db) => {
                    say(&format!("↶ drop {db}"));
                    self.pg.drop_database(&db)
                }
                Undo::RestoreEnv { path, content } => {
                    say(&format!("↶ restore {}", path.display()));
                    env_restored = true;
                    std::fs::write(&path, content).map_err(Into::into)
                }
                Undo::RenameBack { from, to } => {
                    say(&format!("↶ rename {from} → {to}"));
                    self.pg.rename_database(&from, &to)
                }
                Undo::StartServices(s) => {
                    restart = Some(s);
                    Ok(())
                }
                Undo::DeleteBranchRow(id) => self.reg.delete_branch(id),
                Undo::DeleteSnapshotRow(id) => self.reg.delete_snapshot(id),
                Undo::RestoreFiles { branch_id, files } => {
                    self.reg.set_branch_files(branch_id, &files)
                }
                Undo::Relink {
                    id,
                    db_name,
                    git_branch,
                } => self.reg.relink_branch(id, &db_name, &git_branch),
            };
            if let Err(e) = r {
                undo_errors.push(format!("{e:#}"));
            }
        }
        // Services stopped by the plan, or recreated with an env that was just restored.
        let services = restart.or_else(|| env_restored.then(|| self.cfg.routing.restart.clone()));
        if let Some(s) = services.filter(|s| !s.is_empty()) {
            say(&format!("↶ recreate {}", s.join(", ")));
            if let Err(e) = self.compose.recreate(&s) {
                undo_errors.push(format!("{e:#}"));
            }
        }
        Outcome::RolledBack {
            step,
            error,
            undo_errors,
        }
    }

    /// Run one step; returns how to undo it and an optional detail line.
    fn run(&self, step: &Step) -> Result<(Undo, Option<String>)> {
        Ok(match step {
            Step::StopServices { services } => {
                self.compose.stop(services)?;
                (Undo::StartServices(services.clone()), None)
            }
            Step::TerminateConnections { db, .. } => {
                let n = self.pg.terminate_connections(db)?;
                (Undo::Nothing, Some(format!("closed {n} connection(s)")))
            }
            Step::CloneDatabase { source, target, .. } => {
                let method = self.pg.clone_database(source, target)?;
                (
                    Undo::DropDb(target.clone()),
                    Some(format!("copied using {method:?}")),
                )
            }
            Step::CreateEmptyDatabase { name } => {
                self.pg.create_empty(name)?;
                (Undo::DropDb(name.clone()), None)
            }
            Step::FreezeDatabase { name } => {
                self.pg.set_allow_connections(name, false)?;
                (Undo::Nothing, None)
            }
            Step::DropDatabase { name } => {
                self.pg.drop_database(name)?;
                (Undo::Barrier(format!("dropped {name}")), None)
            }
            Step::DropTemporaryDatabase { name } => {
                self.pg.drop_database(name)?;
                (Undo::Nothing, None)
            }
            Step::RenameDatabase { from, to } => {
                self.pg.rename_database(from, to)?;
                (
                    Undo::RenameBack {
                        from: to.clone(),
                        to: from.clone(),
                    },
                    None,
                )
            }
            Step::RewriteEnv {
                file,
                variable,
                from_db,
                to_db,
            } => {
                let path = self.root.join(file);
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let current =
                    envfile::get(&content, variable).and_then(|v| envfile::db_name_in_value(&v));
                if current.as_deref() != Some(from_db.as_str()) {
                    bail!(
                        "{variable} in {file} changed since the plan was made (expected {from_db}, found {})",
                        current.unwrap_or_else(|| "nothing".into())
                    );
                }
                let (updated, _) = envfile::set_db_name(&content, variable, to_db)?;
                std::fs::write(&path, updated)?;
                (Undo::RestoreEnv { path, content }, None)
            }
            Step::SetProxyTarget {
                database,
                from_db,
                to_db,
            } => {
                let current = crate::proxy::read_target(&self.proxy_target, database);
                if &current != from_db {
                    bail!(
                        "the proxy target changed since the plan was made (expected {from_db}, found {current})"
                    );
                }
                crate::proxy::write_target(&self.proxy_target, to_db)?;
                (
                    Undo::RestoreEnv {
                        path: self.proxy_target.clone(),
                        content: format!("{from_db}\n"),
                    },
                    None,
                )
            }
            Step::StartServices { services } => {
                self.compose.recreate(services)?;
                (Undo::Nothing, None)
            }
            Step::RunMigrations { command, .. } | Step::RunSeed { command } => {
                process::shell(command, self.root)?;
                (Undo::Nothing, None)
            }
            Step::RegisterBranch {
                git_branch,
                db_name,
                parent_db,
                base_commit,
                migration_files,
            } => {
                let parent_id = match parent_db {
                    Some(p) => self.reg.branch_by_db(self.project.id, p)?.map(|b| b.id),
                    None => None,
                };
                let b = self.reg.insert_branch(
                    self.project.id,
                    &NewBranch {
                        git_branch: git_branch.clone(),
                        db_name: db_name.clone(),
                        parent_id,
                        base_commit: base_commit.clone(),
                        migration_files: migration_files.clone(),
                        adopted: false,
                    },
                )?;
                (Undo::DeleteBranchRow(b.id), None)
            }
            Step::RecordMigrations {
                db_name,
                migration_files,
            } => {
                let b = self.branch(db_name)?;
                self.reg.set_branch_files(b.id, migration_files)?;
                (
                    Undo::RestoreFiles {
                        branch_id: b.id,
                        files: b.migration_files,
                    },
                    None,
                )
            }
            Step::TouchBranch { db_name } => {
                self.reg.touch_branch(self.branch(db_name)?.id)?;
                (Undo::Nothing, None)
            }
            Step::RegisterSnapshot {
                branch_db,
                db_name,
                seq,
                label,
                commit_id,
                migration_files,
                size_bytes,
            } => {
                let b = self.branch(branch_db)?;
                let s = self.reg.insert_snapshot(&NewSnapshot {
                    branch_id: b.id,
                    seq: *seq,
                    label: label.clone(),
                    db_name: db_name.clone(),
                    commit_id: commit_id.clone(),
                    migration_files: migration_files.clone(),
                    size_bytes: Some(*size_bytes),
                })?;
                (Undo::DeleteSnapshotRow(s.id), None)
            }
            Step::ForgetBranch { db_name } => {
                if let Some(b) = self.reg.branch_by_db(self.project.id, db_name)? {
                    self.reg.delete_branch(b.id)?;
                }
                (Undo::Nothing, None)
            }
            Step::ForgetSnapshot { db_name } => {
                for s in self.reg.snapshots(self.project.id)? {
                    if &s.db_name == db_name {
                        self.reg.delete_snapshot(s.id)?;
                    }
                }
                (Undo::Nothing, None)
            }
            Step::RelinkBranch {
                db_name,
                new_db_name,
                new_git_branch,
            } => {
                let b = self.branch(db_name)?;
                self.reg.relink_branch(b.id, new_db_name, new_git_branch)?;
                (
                    Undo::Relink {
                        id: b.id,
                        db_name: b.db_name,
                        git_branch: b.git_branch,
                    },
                    None,
                )
            }
        })
    }

    fn branch(&self, db_name: &str) -> Result<crate::registry::DbBranch> {
        self.reg
            .branch_by_db(self.project.id, db_name)?
            .with_context(|| format!("{db_name} isn't in the registry"))
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlanKind;

    /// Needs SIDETRACK_TEST_PG (see engine tests).
    #[test]
    fn failed_step_undoes_the_clone() {
        let Ok(argv) = std::env::var("SIDETRACK_TEST_PG") else {
            eprintln!("skipped: SIDETRACK_TEST_PG not set");
            return;
        };
        let pg = Postgres::new(argv.split_whitespace().map(String::from).collect(), None);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "DATABASE_URL=postgres://u@db/other_db\n",
        )
        .unwrap();
        let reg = Registry::open_in_memory().unwrap();
        let project = reg.upsert_project("/t", "t", "postgres", "db").unwrap();
        let cfg: Config = toml::from_str(
            r#"
            [database]
            engine = "postgres"
            service = "db"
            user = "postgres"
            [routing]
            mode = "env"
            file = ".env"
            variable = "DATABASE_URL"
            restart = []
            [migrations]
            tool = "generic"
            dirs = ["m"]
            command = "true"
        "#,
        )
        .unwrap();
        let compose = Compose::new(dir.path(), vec![]);
        let applier = Applier {
            cfg: &cfg,
            root: dir.path(),
            compose: &compose,
            pg: &pg,
            reg: &reg,
            project: &project,
            proxy_target: dir.path().join("proxy-target"),
        };

        pg.drop_database("st_apply_src_db").unwrap();
        pg.drop_database("st_apply_dst_db").unwrap();
        pg.create_empty("st_apply_src_db").unwrap();

        let mut plan = Plan::new(PlanKind::Switch, "test");
        plan.push(Step::CloneDatabase {
            source: "st_apply_src_db".into(),
            target: "st_apply_dst_db".into(),
            size_bytes: 0,
        });
        // The env file points at other_db, not st_apply_src_db, so this step fails.
        plan.push(Step::RewriteEnv {
            file: ".env".into(),
            variable: "DATABASE_URL".into(),
            from_db: "st_apply_src_db".into(),
            to_db: "st_apply_dst_db".into(),
        });
        let report = applier.apply(&plan, &mut |_| {}).unwrap();
        assert!(
            matches!(report.outcome, Outcome::RolledBack { .. }),
            "{:?}",
            report.outcome
        );
        assert!(!pg.exists("st_apply_dst_db").unwrap(), "clone was undone");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).unwrap(),
            "DATABASE_URL=postgres://u@db/other_db\n"
        );
        pg.drop_database("st_apply_src_db").unwrap();
    }
}
