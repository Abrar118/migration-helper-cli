//! Builds plans. Planning reads state but never changes anything.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::engine::postgres::DbInfo;
use crate::git::Git;
use crate::migrations::{self, Candidate, CandidateKind};
use crate::naming;
use crate::plan::{Plan, PlanKind, Step};
use crate::readers::Reading;
use crate::registry::{DbBranch, FileSet, Project, Snapshot};

/// Everything the planner needs to know, gathered up front.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub project: Project,
    pub branches: Vec<DbBranch>,
    pub snapshots: Vec<Snapshot>,
    /// Databases that actually exist on the server.
    pub server_dbs: BTreeMap<String, DbInfo>,
    /// Open sessions per database (application names).
    pub connections: BTreeMap<String, Vec<String>>,
    /// The database the app currently points at, read from the env file.
    pub active_db: Option<String>,
    pub running_services: Vec<String>,
    /// The env file is tracked by git (sidetrack refuses to rewrite it).
    pub env_file_tracked: bool,
    /// Disk space where the database container keeps its data, if known.
    #[serde(default)]
    pub disk: Option<DiskSpace>,
    /// Proxy mode: whether the proxy is accepting connections. `None` in env mode.
    #[serde(default)]
    pub proxy_running: Option<bool>,
    /// What each database's version table says (only for tools with a reader).
    /// When present, it replaces sidetrack's own records as the truth.
    #[serde(default)]
    pub readings: BTreeMap<String, Reading>,
    /// The version table read, e.g. "alembic_version".
    #[serde(default)]
    pub version_table: Option<String>,
    /// Why reading the version tables failed, if it did.
    #[serde(default)]
    pub reader_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResetTo {
    /// Latest snapshot of the active branch, else its parent.
    Latest,
    Snapshot(String),
    Parent,
}

pub struct Planner<'a> {
    pub cfg: &'a Config,
    pub git: &'a Git,
    pub world: &'a World,
}

/// What a DB branch holds (see `Planner::db_state`).
struct DbState {
    applied: FileSet,
    may_contain: FileSet,
    unknown: Vec<String>,
    from_table: bool,
}

/// Applied / pending / unknown migrations for one DB branch, for `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationStatus {
    pub db_name: String,
    /// "per alembic_version", or "per sidetrack's records".
    pub source: String,
    pub applied: usize,
    pub pending: Vec<String>,
    pub unknown: Vec<String>,
}

/// The source a new database is copied from.
struct Base {
    db_name: String,
    /// Migrations known to be applied in the base (see `Candidate::applied`).
    applied: FileSet,
    /// Registry branch to record as parent.
    parent_db: Option<String>,
    why: String,
}

impl<'a> Planner<'a> {
    pub fn new(cfg: &'a Config, git: &'a Git, world: &'a World) -> Self {
        Self { cfg, git, world }
    }

    fn live(&self, db: &str) -> bool {
        self.world.server_dbs.contains_key(db)
    }

    fn size(&self, db: &str) -> u64 {
        self.world
            .server_dbs
            .get(db)
            .map(|d| d.size_bytes)
            .unwrap_or(0)
    }

    pub fn branch_for_git(&self, git_branch: &str) -> Option<&'a DbBranch> {
        self.world
            .branches
            .iter()
            .find(|b| b.git_branch == git_branch)
    }

    pub fn branch_for_db(&self, db: &str) -> Option<&'a DbBranch> {
        self.world.branches.iter().find(|b| b.db_name == db)
    }

    fn branch_by_id(&self, id: i64) -> Option<&'a DbBranch> {
        self.world.branches.iter().find(|b| b.id == id)
    }

    fn active(&self) -> Result<&'a str> {
        self.world.active_db.as_deref().with_context(|| {
            if self.cfg.routing.is_proxy() {
                return "can't tell which database the proxy points at".to_string();
            }
            format!(
                "can't read the active database from {} in {}",
                self.cfg.routing.variable, self.cfg.routing.file
            )
        })
    }

    /// App services that are running now (only these get stopped / recreated).
    fn running_app_services(&self) -> Vec<String> {
        self.cfg
            .routing
            .restart
            .iter()
            .filter(|s| self.world.running_services.contains(s))
            .cloned()
            .collect()
    }

    fn ensure_env_writable(&self) -> Result<()> {
        if !self.cfg.routing.is_proxy() && self.world.env_file_tracked {
            bail!(
                "{} is tracked by git, so sidetrack won't rewrite it (the DB name would end up in commits). \
                 Move {} into an untracked env file and update routing.file in sidetrack.toml.",
                self.cfg.routing.file,
                self.cfg.routing.variable
            );
        }
        Ok(())
    }

    /// Registry and server disagreements. Never fixed automatically.
    pub fn reconcile(&self) -> Vec<String> {
        let mut w = vec![];
        for b in &self.world.branches {
            if !self.live(&b.db_name) {
                w.push(format!(
                    "{} (git branch {}) is in the registry but not on the server",
                    b.db_name, b.git_branch
                ));
            }
        }
        for s in &self.world.snapshots {
            if !self.live(&s.db_name) {
                w.push(format!(
                    "snapshot {} is in the registry but not on the server",
                    s.db_name
                ));
            }
        }
        if let Some(active) = &self.world.active_db {
            if !self.live(active) {
                w.push(format!(
                    "the app points at {active}, which doesn't exist on the server"
                ));
            } else if self.branch_for_db(active).is_none() {
                w.push(format!(
                    "the app points at {active}, which no git branch is linked to"
                ));
            }
        }
        w
    }

    /// Migration files a git branch expects.
    fn target_files(&self, git_branch: &str) -> Result<FileSet> {
        let dirs = &self.cfg.migrations.dirs;
        if self.git.current_branch()?.as_deref() == Some(git_branch) {
            migrations::current_files(self.git, dirs)
        } else if self.git.branch_exists(git_branch)? {
            migrations::files_at(self.git, git_branch, dirs)
        } else {
            bail!("git branch {git_branch} doesn't exist")
        }
    }

    /// Live databases a new branch could be copied from.
    /// What a live DB branch holds. From its version table when it was read;
    /// otherwise from sidetrack's records plus its git branch.
    fn db_state(&self, b: &DbBranch) -> Result<DbState> {
        if let Some(r) = self.world.readings.get(&b.db_name) {
            let mut may_contain = r.files.clone();
            // Rows that match no known file can't be in any target, which makes
            // this database incompatible with every branch (as it should be).
            may_contain.extend(r.unknown.iter().map(|u| format!("?unknown/{u}")));
            return Ok(DbState {
                applied: r.files.clone(),
                may_contain,
                unknown: r.unknown.clone(),
                from_table: true,
            });
        }
        // Over-count: what's recorded plus what its git branch now has.
        // A larger set only makes the candidate less likely to be picked.
        let mut may_contain = b.migration_files.clone();
        if self.git.branch_exists(&b.git_branch)? {
            may_contain.extend(migrations::files_at(
                self.git,
                &b.git_branch,
                &self.cfg.migrations.dirs,
            )?);
        }
        Ok(DbState {
            applied: b.migration_files.clone(),
            may_contain,
            unknown: vec![],
            from_table: false,
        })
    }

    /// Where migration facts come from, for messages.
    fn source(&self, from_table: bool) -> String {
        match (&self.world.version_table, from_table) {
            (Some(t), true) => format!("per {t}"),
            _ => "per sidetrack's records".to_string(),
        }
    }

    /// Applied / pending / unknown migrations for a git branch's DB branch.
    pub fn migration_status(&self, git_branch: &str) -> Result<Option<MigrationStatus>> {
        let Some(b) = self
            .branch_for_git(git_branch)
            .filter(|b| self.live(&b.db_name))
        else {
            return Ok(None);
        };
        let target = self.target_files(git_branch)?;
        let state = self.db_state(b)?;
        let mut unknown = migrations::unknown(&target, &state.applied);
        unknown.extend(state.unknown.iter().cloned());
        Ok(Some(MigrationStatus {
            db_name: b.db_name.clone(),
            source: self.source(state.from_table),
            applied: state.applied.len(),
            pending: migrations::pending(&target, &state.applied),
            unknown,
        }))
    }

    fn candidates(&self) -> Result<Vec<Candidate>> {
        let mut out = vec![];
        for b in &self.world.branches {
            if !self.live(&b.db_name) {
                continue;
            }
            let state = self.db_state(b)?;
            out.push(Candidate {
                db_name: b.db_name.clone(),
                kind: CandidateKind::Branch,
                git_branch: b.git_branch.clone(),
                files: state.may_contain,
                applied: state.applied,
                recency: b.last_used_at,
            });
        }
        for s in &self.world.snapshots {
            if !self.live(&s.db_name) {
                continue;
            }
            let owner = self.branch_by_id(s.branch_id);
            out.push(Candidate {
                db_name: s.db_name.clone(),
                kind: CandidateKind::Snapshot,
                git_branch: owner.map(|b| b.git_branch.clone()).unwrap_or_default(),
                files: s.migration_files.clone(),
                applied: s.migration_files.clone(),
                recency: s.created_at,
            });
        }
        Ok(out)
    }

    fn auto_base(&self, target: &FileSet) -> Result<Option<Base>> {
        let cands = self.candidates()?;
        Ok(migrations::select_base(target, &cands).map(|c| {
            let parent_db = match c.kind {
                CandidateKind::Branch => Some(c.db_name.clone()),
                CandidateKind::Snapshot => self.branch_for_git(&c.git_branch).map(|b| b.db_name.clone()),
            };
            let what = match c.kind {
                CandidateKind::Branch => format!("DB branch of {}", c.git_branch),
                CandidateKind::Snapshot => format!("snapshot of {}", c.git_branch),
            };
            let from_table = c.kind == CandidateKind::Branch && self.world.readings.contains_key(&c.db_name);
            Base {
                db_name: c.db_name.clone(),
                applied: c.applied.clone(),
                parent_db,
                why: format!(
                    "{} is the closest compatible base ({what}; holds no migration this branch lacks; \
                     {} of {} migrations applied {})",
                    c.db_name,
                    c.applied.len(),
                    target.len(),
                    self.source(from_table)
                ),
            }
        }))
    }

    /// `--from` accepts a git branch, a DB branch name or a snapshot name/label.
    fn explicit_base(&self, from: &str) -> Result<Base> {
        if let Some(b) = self
            .branch_for_git(from)
            .or_else(|| self.branch_for_db(from))
        {
            anyhow::ensure!(
                self.live(&b.db_name),
                "{} doesn't exist on the server",
                b.db_name
            );
            return Ok(Base {
                db_name: b.db_name.clone(),
                applied: self.db_state(b)?.applied,
                parent_db: Some(b.db_name.clone()),
                why: format!("copying {} as requested", b.db_name),
            });
        }
        let s = self.find_snapshot(from)?;
        Ok(Base {
            db_name: s.db_name.clone(),
            applied: s.migration_files.clone(),
            parent_db: self.branch_by_id(s.branch_id).map(|b| b.db_name.clone()),
            why: format!("copying snapshot {} as requested", s.db_name),
        })
    }

    fn find_snapshot(&self, name: &str) -> Result<&'a Snapshot> {
        self.world
            .snapshots
            .iter()
            .find(|s| s.db_name == name || s.label.as_deref() == Some(name))
            .with_context(|| format!("no DB branch or snapshot called {name}"))
    }

    /// DB name for a new branch, avoiding names used by other branches or unknown databases.
    fn choose_name(&self, git_branch: &str) -> Result<String> {
        let taken = |name: &str| {
            self.world
                .branches
                .iter()
                .any(|b| b.db_name == name && b.git_branch != git_branch)
                || (self.live(name) && self.branch_for_db(name).is_none())
                || self.world.snapshots.iter().any(|s| s.db_name == name)
        };
        let plain = naming::db_name_for_branch(git_branch);
        if !taken(&plain) {
            return Ok(plain);
        }
        let hashed = naming::hashed_db_name(git_branch);
        if !taken(&hashed) {
            return Ok(hashed);
        }
        bail!("both {plain} and {hashed} are already in use by other databases")
    }

    // ------------------------------------------------------------------ switch

    pub fn switch(&self, git_branch: &str) -> Result<Plan> {
        let active = self.active()?.to_string();
        let target = self.target_files(git_branch)?;
        let mut plan = Plan::new(PlanKind::Switch, String::new());
        plan.warnings = self.reconcile();

        match self.branch_for_git(git_branch) {
            Some(b) if self.live(&b.db_name) => {
                let state = self.db_state(b)?;
                let mut unknown = migrations::unknown(&target, &state.applied);
                unknown.extend(state.unknown.iter().cloned());
                if !unknown.is_empty() {
                    let names: Vec<&str> = unknown
                        .iter()
                        .map(|u| u.rsplit('/').next().unwrap_or(u))
                        .collect();
                    plan.warnings.push(format!(
                        "{} has {} migration(s) that {git_branch} doesn't have ({}, {}); \
                         consider `sidetrack reset` if your migration tool complains",
                        b.db_name,
                        unknown.len(),
                        names.join(", "),
                        self.source(state.from_table)
                    ));
                }
                let pending = migrations::pending(&target, &state.applied);
                if b.db_name == active {
                    plan.summary = format!("{git_branch} is already on {}", b.db_name);
                    if !pending.is_empty() {
                        self.push_migrations(&mut plan, &b.db_name, pending, &target);
                    }
                } else {
                    self.ensure_env_writable()?;
                    plan.summary =
                        format!("Switch DB from {active} to {} for {git_branch}", b.db_name);
                    // The proxy needs no restart; env mode recreates the app.
                    let restart = if self.cfg.routing.is_proxy() {
                        vec![]
                    } else {
                        self.running_app_services()
                    };
                    self.push_route(&mut plan, &active, &b.db_name, &restart);
                    if !pending.is_empty() {
                        self.push_migrations(&mut plan, &b.db_name, pending, &target);
                    }
                }
                plan.push(Step::TouchBranch {
                    db_name: b.db_name.clone(),
                });
            }
            existing => {
                self.ensure_env_writable()?;
                if let Some(b) = existing {
                    plan.push(Step::ForgetBranch {
                        db_name: b.db_name.clone(),
                    });
                }
                let name = match existing {
                    Some(b) if !self.live(&b.db_name) => b.db_name.clone(),
                    _ => self.choose_name(git_branch)?,
                };
                plan.summary =
                    format!("Create {name} for {git_branch} and switch to it (from {active})");
                self.push_new_branch(
                    &mut plan, git_branch, &name, &active, &target, None, false, true,
                )?;
            }
        }
        self.guard_disk(plan)
    }

    /// Point the app at `to`, then start `services` (env mode: the running app
    /// services; proxy mode: only those stopped for a copy).
    fn push_route(&self, plan: &mut Plan, from: &str, to: &str, services: &[String]) {
        plan.push(self.route_step(from, to));
        if self.cfg.routing.is_proxy() {
            if self.world.proxy_running == Some(false) {
                plan.warnings.push(
                    "the proxy isn't running, so the app can't reach any database; start it with `sidetrack proxy`"
                        .to_string(),
                );
            }
            if !services.is_empty() {
                plan.push(Step::StartServices {
                    services: services.to_vec(),
                });
            }
        } else if services.is_empty() {
            plan.notes.push(format!(
                "none of {} is running; start them yourself to use the new database",
                self.cfg.routing.restart.join(", ")
            ));
        } else {
            plan.push(Step::StartServices {
                services: services.to_vec(),
            });
        }
    }

    fn route_step(&self, from: &str, to: &str) -> Step {
        if self.cfg.routing.is_proxy() {
            Step::SetProxyTarget {
                database: self.cfg.routing.database.clone().unwrap_or_default(),
                from_db: from.to_string(),
                to_db: to.to_string(),
            }
        } else {
            Step::RewriteEnv {
                file: self.cfg.routing.file.clone(),
                variable: self.cfg.routing.variable.clone(),
                from_db: from.to_string(),
                to_db: to.to_string(),
            }
        }
    }

    fn push_migrations(&self, plan: &mut Plan, db: &str, pending: Vec<String>, target: &FileSet) {
        if self.running_app_services().is_empty() && self.cfg.migrations.command.contains(" exec ")
        {
            plan.warnings.push(format!(
                "migrations run with `docker compose exec`, but {} isn't running",
                self.cfg.routing.restart.join(", ")
            ));
        }
        plan.push(Step::RunMigrations {
            command: self.cfg.migrations.command.clone(),
            pending,
        });
        plan.push(Step::RecordMigrations {
            db_name: db.to_string(),
            migration_files: target.clone(),
        });
    }

    /// Copy (or create) a database for `git_branch`, register it and optionally switch to it.
    #[allow(clippy::too_many_arguments)]
    fn push_new_branch(
        &self,
        plan: &mut Plan,
        git_branch: &str,
        name: &str,
        active: &str,
        target: &FileSet,
        from: Option<&str>,
        empty: bool,
        switch_to: bool,
    ) -> Result<()> {
        let base = if empty {
            None
        } else if let Some(f) = from {
            Some(self.explicit_base(f)?)
        } else {
            self.auto_base(target)?
        };
        let base_commit = if self.git.branch_exists(git_branch)? {
            self.git.resolve(git_branch)?
        } else {
            None
        };
        let running = self.running_app_services();
        let mut stopped = false;

        let applied = match &base {
            Some(base) => {
                plan.notes.push(base.why.clone());
                let stop = base.db_name == active && !running.is_empty();
                stopped = stop;
                if stop {
                    plan.push(Step::StopServices {
                        services: running.clone(),
                    });
                }
                self.push_terminate(plan, &base.db_name, stop);
                plan.push(Step::CloneDatabase {
                    source: base.db_name.clone(),
                    target: name.to_string(),
                    size_bytes: self.size(&base.db_name),
                });
                plan.push(Step::RegisterBranch {
                    git_branch: git_branch.to_string(),
                    db_name: name.to_string(),
                    parent_db: base.parent_db.clone(),
                    base_commit,
                    migration_files: base.applied.clone(),
                });
                if stop && !switch_to {
                    plan.push(Step::StartServices {
                        services: running.clone(),
                    });
                }
                base.applied.clone()
            }
            None => {
                if !empty {
                    plan.warnings.push(
                        "no existing database is compatible with this branch, so it will be built from empty"
                            .to_string(),
                    );
                }
                plan.push(Step::CreateEmptyDatabase {
                    name: name.to_string(),
                });
                plan.push(Step::RegisterBranch {
                    git_branch: git_branch.to_string(),
                    db_name: name.to_string(),
                    parent_db: None,
                    base_commit,
                    migration_files: FileSet::new(),
                });
                FileSet::new()
            }
        };

        if switch_to {
            let restart = if self.cfg.routing.is_proxy() && !stopped {
                vec![]
            } else {
                running
            };
            self.push_route(plan, active, name, &restart);
            let pending = migrations::pending(target, &applied);
            if !pending.is_empty() {
                self.push_migrations(plan, name, pending, target);
            }
            if base.is_none() {
                if let Some(seed) = &self.cfg.migrations.seed {
                    plan.push(Step::RunSeed {
                        command: seed.clone(),
                    });
                }
            }
            plan.push(Step::TouchBranch {
                db_name: name.to_string(),
            });
        } else if !migrations::pending(target, &applied).is_empty() {
            plan.notes.push(format!(
                "{} migration(s) will run when you switch to {git_branch}",
                migrations::pending(target, &applied).len()
            ));
        }
        Ok(())
    }

    /// Show open sessions that will be closed. After stopping the app, its own
    /// sessions go away; anything else (a psql shell, a GUI client) is listed.
    fn push_terminate(&self, plan: &mut Plan, db: &str, app_stopped: bool) {
        if let Some(sessions) = self.world.connections.get(db) {
            if !sessions.is_empty() {
                plan.push(Step::TerminateConnections {
                    db: db.to_string(),
                    sessions: sessions.clone(),
                });
                if app_stopped {
                    plan.notes
                        .push("sessions from the stopped services close on their own".to_string());
                }
            }
        }
    }

    // ------------------------------------------------------------------ create

    pub fn create(&self, git_branch: &str, from: Option<&str>, empty: bool) -> Result<Plan> {
        if let Some(b) = self.branch_for_git(git_branch) {
            if self.live(&b.db_name) {
                bail!("{git_branch} already has a DB branch ({})", b.db_name);
            }
        }
        let active = self.active()?.to_string();
        let target = self.target_files(git_branch)?;
        let name = self.choose_name(git_branch)?;
        let mut plan = Plan::new(PlanKind::Create, format!("Create {name} for {git_branch}"));
        plan.warnings = self.reconcile();
        if let Some(b) = self.branch_for_git(git_branch) {
            plan.push(Step::ForgetBranch {
                db_name: b.db_name.clone(),
            });
        }
        self.push_new_branch(
            &mut plan, git_branch, &name, &active, &target, from, empty, false,
        )?;
        self.guard_disk(plan)
    }

    // ---------------------------------------------------------------- snapshot

    fn active_branch(&self) -> Result<&'a DbBranch> {
        let active = self.active()?;
        self.branch_for_db(active).with_context(|| {
            format!(
                "{active} isn't linked to a git branch; run `sidetrack init` or `sidetrack switch`"
            )
        })
    }

    pub fn snapshot(&self, label: Option<&str>, next_seq: i64) -> Result<Plan> {
        let b = self.active_branch()?;
        if let Some(l) = label {
            anyhow::ensure!(
                self.world
                    .snapshots
                    .iter()
                    .all(|s| s.label.as_deref() != Some(l)),
                "a snapshot labelled {l:?} already exists"
            );
        }
        let name = naming::snapshot_db_name(&b.db_name, next_seq);
        let mut plan = Plan::new(
            PlanKind::Snapshot,
            format!("Snapshot {} as {name}", b.db_name),
        );
        plan.warnings = self.reconcile();
        let running = self.running_app_services();
        if !running.is_empty() {
            plan.push(Step::StopServices {
                services: running.clone(),
            });
        }
        self.push_terminate(&mut plan, &b.db_name, !running.is_empty());
        plan.push(Step::CloneDatabase {
            source: b.db_name.clone(),
            target: name.clone(),
            size_bytes: self.size(&b.db_name),
        });
        plan.push(Step::FreezeDatabase { name: name.clone() });
        plan.push(Step::RegisterSnapshot {
            branch_db: b.db_name.clone(),
            db_name: name,
            seq: next_seq,
            label: label.map(str::to_string),
            commit_id: self.git.head_commit().ok(),
            migration_files: self.db_state(b)?.applied,
            size_bytes: self.size(&b.db_name),
        });
        if !running.is_empty() {
            plan.push(Step::StartServices { services: running });
        }
        self.guard_disk(plan)
    }

    pub fn snapshot_remove(&self, name: &str) -> Result<Plan> {
        let s = self.find_snapshot(name)?;
        let mut plan = Plan::new(
            PlanKind::SnapshotRemove,
            format!("Delete snapshot {}", s.db_name),
        );
        if self.live(&s.db_name) {
            plan.push(Step::DropDatabase {
                name: s.db_name.clone(),
            });
        }
        plan.push(Step::ForgetSnapshot {
            db_name: s.db_name.clone(),
        });
        Ok(plan)
    }

    // ------------------------------------------------------------------- reset

    pub fn reset(&self, to: &ResetTo) -> Result<Plan> {
        let b = self.active_branch()?;
        let own_latest = || {
            self.world
                .snapshots
                .iter()
                .find(|s| s.branch_id == b.id && self.live(&s.db_name))
        };
        let parent = || {
            b.parent_id
                .and_then(|id| self.branch_by_id(id))
                .filter(|p| self.live(&p.db_name))
        };
        let (source, files) = match to {
            ResetTo::Snapshot(n) => {
                let s = self.find_snapshot(n)?;
                (s.db_name.clone(), s.migration_files.clone())
            }
            ResetTo::Parent => {
                let p = parent().with_context(|| {
                    format!("{} has no parent DB branch on the server", b.db_name)
                })?;
                (p.db_name.clone(), p.migration_files.clone())
            }
            ResetTo::Latest => match (own_latest(), parent()) {
                (Some(s), _) => (s.db_name.clone(), s.migration_files.clone()),
                (None, Some(p)) => (p.db_name.clone(), p.migration_files.clone()),
                (None, None) => bail!("{} has no snapshots and no parent to reset to", b.db_name),
            },
        };
        anyhow::ensure!(source != b.db_name, "can't reset {} to itself", b.db_name);
        anyhow::ensure!(self.live(&source), "{source} doesn't exist on the server");

        let tmp = naming::temp_db_name(&b.db_name);
        let mut plan = Plan::new(
            PlanKind::Reset,
            format!("Reset {} to a fresh copy of {source}", b.db_name),
        );
        plan.warnings = self.reconcile();
        if self.live(&tmp) {
            plan.warnings.push(format!(
                "{tmp} is left over from an earlier reset and will be replaced"
            ));
            plan.push(Step::DropDatabase { name: tmp.clone() });
        }
        let running = self.running_app_services();
        if !running.is_empty() {
            plan.push(Step::StopServices {
                services: running.clone(),
            });
        }
        self.push_terminate(&mut plan, &source, false);
        plan.push(Step::CloneDatabase {
            source: source.clone(),
            target: tmp.clone(),
            size_bytes: self.size(&source),
        });
        self.push_terminate(&mut plan, &b.db_name, !running.is_empty());
        plan.push(Step::DropDatabase {
            name: b.db_name.clone(),
        });
        plan.push(Step::RenameDatabase {
            from: tmp,
            to: b.db_name.clone(),
        });
        plan.push(Step::RecordMigrations {
            db_name: b.db_name.clone(),
            migration_files: files.clone(),
        });
        if !running.is_empty() {
            plan.push(Step::StartServices { services: running });
        }
        if let Some(current) = self.git.current_branch()? {
            if current == b.git_branch {
                let target = self.target_files(&current)?;
                let pending = migrations::pending(&target, &files);
                if !pending.is_empty() {
                    self.push_migrations(&mut plan, &b.db_name, pending, &target);
                }
            }
        }
        plan.push(Step::TouchBranch {
            db_name: b.db_name.clone(),
        });
        self.guard_disk(plan)
    }

    // ------------------------------------------------------------------ delete

    pub fn delete(&self, name: &str) -> Result<Plan> {
        let b = self
            .branch_for_git(name)
            .or_else(|| self.branch_for_db(name))
            .with_context(|| format!("no DB branch for {name}"))?;
        if self.world.active_db.as_deref() == Some(b.db_name.as_str()) {
            bail!(
                "{} is the active database; switch to another branch first",
                b.db_name
            );
        }
        let mut plan = Plan::new(
            PlanKind::Delete,
            format!("Delete {} (git branch {})", b.db_name, b.git_branch),
        );
        if b.adopted {
            plan.warnings.push(format!(
                "{} existed before sidetrack (it was adopted at init)",
                b.db_name
            ));
        }
        self.push_delete_branch(&mut plan, b);
        Ok(plan)
    }

    fn push_delete_branch(&self, plan: &mut Plan, b: &DbBranch) {
        for s in self.world.snapshots.iter().filter(|s| s.branch_id == b.id) {
            if self.live(&s.db_name) {
                plan.push(Step::DropDatabase {
                    name: s.db_name.clone(),
                });
            }
            plan.push(Step::ForgetSnapshot {
                db_name: s.db_name.clone(),
            });
        }
        if self.live(&b.db_name) {
            plan.push(Step::DropDatabase {
                name: b.db_name.clone(),
            });
        }
        plan.push(Step::ForgetBranch {
            db_name: b.db_name.clone(),
        });
    }

    // ---------------------------------------------------------------------- gc

    pub fn gc(&self) -> Result<Plan> {
        let mut plan = Plan::new(
            PlanKind::Gc,
            "Clean up DB branches whose git branch is gone or merged".to_string(),
        );
        let merged: Vec<String> = match &self.cfg.git.main_branch {
            Some(main) if self.git.branch_exists(main)? => self.git.merged_into(main)?,
            _ => vec![],
        };
        let mut freed = 0;
        for b in &self.world.branches {
            if self.world.active_db.as_deref() == Some(b.db_name.as_str()) {
                continue;
            }
            let gone = !self.git.branch_exists(&b.git_branch)?;
            // A branch with no commits of its own also counts as "merged" to git, so
            // only treat it as merged if it moved since its DB branch was created.
            let moved = || -> Result<bool> {
                Ok(self.git.resolve(&b.git_branch)?.as_deref() != b.base_commit.as_deref())
            };
            let is_merged = merged.contains(&b.git_branch) && !b.adopted && moved()?;
            if !(gone || is_merged || !self.live(&b.db_name)) {
                continue;
            }
            let why = if !self.live(&b.db_name) {
                "database is missing"
            } else if gone {
                "git branch deleted"
            } else {
                "merged"
            };
            plan.notes
                .push(format!("{} ({}): {why}", b.db_name, b.git_branch));
            freed += self.size(&b.db_name);
            for s in self.world.snapshots.iter().filter(|s| s.branch_id == b.id) {
                freed += self.size(&s.db_name);
            }
            self.push_delete_branch(&mut plan, b);
        }
        if let Some(keep) = self.cfg.snapshots.keep {
            self.push_retention(&mut plan, keep, &mut freed);
        }
        for s in &self.world.snapshots {
            if !self.live(&s.db_name)
                && !plan.steps.iter().any(
                    |st| matches!(st, Step::ForgetSnapshot { db_name } if *db_name == s.db_name),
                )
            {
                plan.push(Step::ForgetSnapshot {
                    db_name: s.db_name.clone(),
                });
            }
        }
        if freed > 0 {
            plan.notes.push(format!(
                "frees about {}",
                crate::engine::postgres::format_bytes(freed)
            ));
        }
        Ok(plan)
    }

    /// Drop unlabelled snapshots beyond the newest `keep` per branch (`snapshots.keep`).
    fn push_retention(&self, plan: &mut Plan, keep: usize, freed: &mut u64) {
        let dropping = |db: &str| {
            plan.steps
                .iter()
                .any(|st| matches!(st, Step::DropDatabase { name } if name == db))
        };
        let mut extra = vec![];
        for b in &self.world.branches {
            // `world.snapshots` is newest first.
            let old = self
                .world
                .snapshots
                .iter()
                .filter(|s| s.branch_id == b.id && s.label.is_none() && self.live(&s.db_name))
                .skip(keep);
            for s in old {
                if !dropping(&s.db_name) {
                    extra.push(s);
                }
            }
        }
        if !extra.is_empty() {
            plan.notes.push(format!(
                "{} old snapshot(s) beyond snapshots.keep = {keep} (labelled snapshots are kept)",
                extra.len()
            ));
        }
        for s in extra {
            *freed += self.size(&s.db_name);
            plan.push(Step::DropDatabase {
                name: s.db_name.clone(),
            });
            plan.push(Step::ForgetSnapshot {
                db_name: s.db_name.clone(),
            });
        }
    }

    // ------------------------------------------------------------------- bench

    /// Time a copy of `from` (default: the active database), then drop the copy.
    pub fn bench(&self, from: Option<&str>) -> Result<Plan> {
        let source = match from {
            None => self.active()?.to_string(),
            Some(name) => self
                .branch_for_git(name)
                .map(|b| b.db_name.clone())
                .unwrap_or_else(|| name.to_string()),
        };
        anyhow::ensure!(self.live(&source), "{source} doesn't exist on the server");
        let copy = naming::bench_db_name(&source);
        let mut plan = Plan::new(
            PlanKind::Bench,
            format!(
                "Time a copy of {source} ({})",
                crate::engine::postgres::format_bytes(self.size(&source))
            ),
        );
        if self.live(&copy) {
            plan.warnings.push(format!(
                "{copy} is left over from an earlier bench and will be dropped first"
            ));
            plan.push(Step::DropTemporaryDatabase { name: copy.clone() });
        }
        let running = self.running_app_services();
        let stop = self.world.active_db.as_deref() == Some(source.as_str()) && !running.is_empty();
        if stop {
            plan.push(Step::StopServices {
                services: running.clone(),
            });
            plan.notes.push(format!(
                "{} is stopped only while the copy runs; use --from <snapshot or other branch> to avoid that",
                running.join(", ")
            ));
        }
        self.push_terminate(&mut plan, &source, stop);
        plan.push(Step::CloneDatabase {
            source: source.clone(),
            target: copy.clone(),
            size_bytes: self.size(&source),
        });
        if stop {
            plan.push(Step::StartServices { services: running });
        }
        plan.push(Step::DropTemporaryDatabase { name: copy });
        self.guard_disk(plan)
    }

    // ------------------------------------------------------------------ rename

    /// Follow a renamed git branch (`git branch -m old new`).
    pub fn rename(&self, old: &str, new: &str) -> Result<Plan> {
        let b = self
            .branch_for_git(old)
            .or_else(|| self.branch_for_db(old))
            .with_context(|| format!("no DB branch for {old}"))?;
        if let Some(other) = self.branch_for_git(new) {
            bail!("{new} already has a DB branch ({})", other.db_name);
        }
        let plain = naming::db_name_for_branch(new);
        // Adopted databases (e.g. the original app_dev) keep their name; only the link moves.
        let new_db = if b.adopted || plain == b.db_name {
            b.db_name.clone()
        } else {
            self.choose_name(new)?
        };
        let mut plan = Plan::new(
            PlanKind::Rename,
            format!(
                "Link {} to git branch {new} (was {})",
                b.db_name, b.git_branch
            ),
        );
        plan.warnings = self.reconcile();
        if new_db != b.db_name {
            anyhow::ensure!(
                self.live(&b.db_name),
                "{} doesn't exist on the server",
                b.db_name
            );
            plan.summary = format!("Rename {} → {new_db} for git branch {new}", b.db_name);
            let active = self.world.active_db.as_deref() == Some(b.db_name.as_str());
            let running = if active {
                self.running_app_services()
            } else {
                vec![]
            };
            if active {
                self.ensure_env_writable()?;
                if !running.is_empty() {
                    plan.push(Step::StopServices {
                        services: running.clone(),
                    });
                }
            }
            self.push_terminate(&mut plan, &b.db_name, !running.is_empty());
            plan.push(Step::RenameDatabase {
                from: b.db_name.clone(),
                to: new_db.clone(),
            });
            if active {
                plan.push(self.route_step(&b.db_name, &new_db));
                if !running.is_empty() {
                    plan.push(Step::StartServices { services: running });
                }
            }
        }
        plan.push(Step::RelinkBranch {
            db_name: b.db_name.clone(),
            new_db_name: new_db,
            new_git_branch: new.to_string(),
        });
        if self.git.branch_exists(&b.git_branch)? && !self.git.branch_exists(new)? {
            plan.notes.push(format!(
                "this only renames the database; rename the git branch with `git branch -m {} {new}`",
                b.git_branch
            ));
        }
        Ok(plan)
    }

    // ------------------------------------------------------------- disk guard

    /// Refuse plans that won't fit on the database container's disk; warn when
    /// they'd leave less than 1 GB or 10% free.
    fn guard_disk(&self, mut plan: Plan) -> Result<Plan> {
        let need = plan.added_bytes();
        let Some(disk) = self.world.disk else {
            return Ok(plan);
        };
        if need == 0 {
            return Ok(plan);
        }
        use crate::engine::postgres::format_bytes;
        if need > disk.free_bytes {
            bail!(
                "not enough disk space for the {} container: this needs about {}, but only {} is free. \
                 Free some with `sidetrack gc` or `sidetrack delete <branch>`",
                self.cfg.database.service,
                format_bytes(need),
                format_bytes(disk.free_bytes)
            );
        }
        let left = disk.free_bytes - need;
        let reserve = (1u64 << 30).max(disk.total_bytes / 10);
        if left < reserve {
            plan.warnings.push(format!(
                "this leaves only {} free on the database's disk (of {}); consider `sidetrack gc`",
                format_bytes(left),
                format_bytes(disk.total_bytes)
            ));
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ComposeConfig, DatabaseConfig, GitConfig, MigrationsConfig, RoutingConfig,
    };
    use crate::git::testutil::*;

    fn cfg() -> Config {
        Config {
            database: DatabaseConfig {
                engine: "postgres".into(),
                service: "db".into(),
                user: "postgres".into(),
                psql: None,
            },
            routing: RoutingConfig {
                mode: "env".into(),
                file: ".env".into(),
                variable: "DATABASE_URL".into(),
                restart: vec!["api".into()],
                database: None,
                listen: None,
                upstream: None,
            },
            migrations: MigrationsConfig {
                tool: "generic".into(),
                dirs: vec!["migrations".into()],
                command: "./migrate".into(),
                seed: Some("./seed".into()),
                version_table: None,
                version_column: None,
            },
            compose: ComposeConfig::default(),
            git: GitConfig {
                main_branch: Some("main".into()),
            },
            snapshots: Default::default(),
        }
    }

    fn files(names: &[&str]) -> FileSet {
        names.iter().map(|n| format!("migrations/{n}")).collect()
    }

    fn branch(id: i64, git: &str, db: &str, f: &[&str], used: i64) -> DbBranch {
        DbBranch {
            id,
            project_id: 1,
            git_branch: git.into(),
            db_name: db.into(),
            parent_id: None,
            base_commit: None,
            migration_files: files(f),
            adopted: false,
            created_at: used,
            last_used_at: used,
        }
    }

    fn world(branches: Vec<DbBranch>, active: &str) -> World {
        let server_dbs = branches
            .iter()
            .map(|b| {
                (
                    b.db_name.clone(),
                    DbInfo {
                        name: b.db_name.clone(),
                        size_bytes: 1000,
                        allow_connections: true,
                    },
                )
            })
            .collect();
        World {
            project: Project {
                id: 1,
                repo_root: "/r".into(),
                compose_project: "r".into(),
                engine: "postgres".into(),
                db_service: "db".into(),
                auto_approve: false,
            },
            branches,
            snapshots: vec![],
            server_dbs,
            connections: BTreeMap::from([(active.to_string(), vec!["api".to_string()])]),
            active_db: Some(active.into()),
            running_services: vec!["api".into()],
            env_file_tracked: false,
            disk: None,
            proxy_running: None,
            readings: BTreeMap::new(),
            version_table: None,
            reader_error: None,
        }
    }

    fn kinds(plan: &Plan) -> Vec<&'static str> {
        plan.steps
            .iter()
            .map(|s| match s {
                Step::StopServices { .. } => "stop",
                Step::TerminateConnections { .. } => "terminate",
                Step::CloneDatabase { .. } => "clone",
                Step::CreateEmptyDatabase { .. } => "create_empty",
                Step::FreezeDatabase { .. } => "freeze",
                Step::DropDatabase { .. } => "drop",
                Step::RenameDatabase { .. } => "rename",
                Step::RewriteEnv { .. } => "env",
                Step::SetProxyTarget { .. } => "proxy",
                Step::StartServices { .. } => "start",
                Step::RunMigrations { .. } => "migrate",
                Step::RunSeed { .. } => "seed",
                Step::RegisterBranch { .. } => "register",
                Step::RecordMigrations { .. } => "record",
                Step::TouchBranch { .. } => "touch",
                Step::RegisterSnapshot { .. } => "register_snapshot",
                Step::ForgetBranch { .. } => "forget",
                Step::ForgetSnapshot { .. } => "forget_snapshot",
                Step::DropTemporaryDatabase { .. } => "drop_temp",
                Step::RelinkBranch { .. } => "relink",
            })
            .collect()
    }

    /// Spec §1: A has 2 migrations, B adds 4 more. Returning to A just switches back.
    fn proxy_cfg() -> Config {
        let mut c = cfg();
        c.routing.mode = "proxy".into();
        c.routing.database = Some("app_dev".into());
        c
    }

    #[test]
    fn proxy_switch_needs_no_restart() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "a"]);
        commit_file(&repo, "migrations/001.sql", "1");
        let mut w = world(
            vec![
                branch(1, "a", "a_db", &["001.sql"], 1),
                branch(2, "b", "b_db", &["001.sql"], 2),
            ],
            "b_db",
        );
        w.env_file_tracked = true; // irrelevant in proxy mode
        w.proxy_running = Some(true);
        let c = proxy_cfg();
        let plan = Planner::new(&c, &repo, &w).switch("a").unwrap();
        assert_eq!(kinds(&plan), ["proxy", "touch"]);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        assert_eq!(
            plan.steps[0].describe(),
            "Route app_dev through the proxy to a_db (was b_db)"
        );

        w.proxy_running = Some(false);
        let plan = Planner::new(&c, &repo, &w).switch("a").unwrap();
        assert!(plan
            .warnings
            .iter()
            .any(|x| x.contains("`sidetrack proxy`")));
    }

    #[test]
    fn proxy_copy_of_active_db_restarts_only_stopped_services() {
        let (_d, repo) = repo();
        commit_file(&repo, "migrations/001.sql", "1");
        git(&repo, &["checkout", "-q", "-b", "c"]);
        let w = world(
            vec![branch(1, "main", "main_db", &["001.sql"], 1)],
            "main_db",
        );
        let c = proxy_cfg();
        let plan = Planner::new(&c, &repo, &w).switch("c").unwrap();
        let k = kinds(&plan);
        assert_eq!(k[0], "stop", "{k:?}");
        let proxy = k.iter().position(|s| *s == "proxy").unwrap();
        assert_eq!(k[proxy + 1], "start", "{k:?}");
        assert!(!k.contains(&"env"));
    }

    #[test]
    fn spec_scenario_switch_back_runs_nothing() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "a"]);
        commit_file(&repo, "migrations/001.sql", "1");
        commit_file(&repo, "migrations/002.sql", "2");
        git(&repo, &["checkout", "-q", "-b", "b"]);
        for n in 3..=6 {
            commit_file(&repo, &format!("migrations/00{n}.sql"), "x");
        }
        git(&repo, &["checkout", "-q", "a"]);

        let w = world(
            vec![
                branch(1, "a", "a_db", &["001.sql", "002.sql"], 1),
                branch(
                    2,
                    "b",
                    "b_db",
                    &[
                        "001.sql", "002.sql", "003.sql", "004.sql", "005.sql", "006.sql",
                    ],
                    2,
                ),
            ],
            "b_db",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("a").unwrap();
        assert_eq!(
            kinds(&plan),
            ["env", "start", "touch"],
            "{}",
            plan.render(true)
        );
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn new_branch_copies_closest_base_and_migrates_forward() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "dev"]);
        commit_file(&repo, "migrations/001.sql", "1");
        git(&repo, &["checkout", "-q", "-b", "feature/auth"]);
        commit_file(&repo, "migrations/002.sql", "2");

        let w = world(vec![branch(1, "dev", "dev_db", &["001.sql"], 1)], "dev_db");
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("feature/auth").unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "stop",
                "terminate",
                "clone",
                "register",
                "env",
                "start",
                "migrate",
                "record",
                "touch"
            ],
            "{}",
            plan.render(true)
        );
        assert!(plan.steps.contains(&Step::CloneDatabase {
            source: "dev_db".into(),
            target: "feature_auth_db".into(),
            size_bytes: 1000
        }));
        assert!(plan.steps.contains(&Step::RunMigrations {
            command: "./migrate".into(),
            pending: vec!["migrations/002.sql".into()]
        }));
    }

    /// Regression (found by the e2e test): main gained a migration in git that its
    /// local database never ran. A branch from main must still run it.
    #[test]
    fn base_behind_its_git_branch_still_migrates() {
        let (_d, repo) = repo();
        commit_file(&repo, "migrations/001.sql", "1");
        commit_file(&repo, "migrations/004.sql", "4");
        git(&repo, &["checkout", "-q", "-b", "feature/c"]);

        let w = world(
            vec![branch(1, "main", "app_dev", &["001.sql"], 1)],
            "app_dev",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("feature/c").unwrap();
        assert!(
            plan.steps.contains(&Step::RunMigrations {
                command: "./migrate".into(),
                pending: vec!["migrations/004.sql".into()]
            }),
            "{}",
            plan.render(true)
        );
    }

    #[test]
    fn no_compatible_base_builds_from_empty_and_seeds() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "other"]);
        commit_file(&repo, "migrations/009.sql", "9");
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["checkout", "-q", "-b", "fresh"]);
        commit_file(&repo, "migrations/001.sql", "1");

        let w = world(
            vec![branch(1, "other", "other_db", &["009.sql"], 1)],
            "other_db",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("fresh").unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "create_empty",
                "register",
                "env",
                "start",
                "migrate",
                "record",
                "seed",
                "touch"
            ]
        );
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn name_collision_uses_hash() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "feature-foo"]);
        let w = world(
            vec![branch(1, "feature/Foo", "feature_foo_db", &[], 1)],
            "feature_foo_db",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("feature-foo").unwrap();
        let target = plan.steps.iter().find_map(|s| match s {
            Step::CloneDatabase { target, .. } => Some(target.clone()),
            _ => None,
        });
        assert_eq!(target.unwrap(), naming::hashed_db_name("feature-foo"));
    }

    #[test]
    fn tracked_env_file_is_refused() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "x"]);
        let mut w = world(vec![branch(1, "main", "main_db", &[], 1)], "main_db");
        w.env_file_tracked = true;
        let c = cfg();
        let err = Planner::new(&c, &repo, &w).switch("x").unwrap_err();
        assert!(format!("{err}").contains("tracked by git"));
    }

    #[test]
    fn reset_clones_before_dropping() {
        let (_d, repo) = repo();
        let mut feature = branch(2, "main", "main_db", &[], 2);
        feature.parent_id = Some(1);
        let w = world(vec![branch(1, "dev", "dev_db", &[], 1), feature], "main_db");
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).reset(&ResetTo::Latest).unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "stop",
                "clone",
                "terminate",
                "drop",
                "rename",
                "record",
                "start",
                "touch"
            ]
        );
        assert!(plan.has_destructive());
    }

    #[test]
    fn delete_refuses_active_and_gc_finds_gone_branches() {
        let (_d, repo) = repo();
        let w = world(
            vec![
                branch(1, "main", "main_db", &[], 1),
                branch(2, "deleted/branch", "deleted_branch_db", &[], 2),
            ],
            "main_db",
        );
        let c = cfg();
        let p = Planner::new(&c, &repo, &w);
        assert!(p.delete("main").is_err());
        let gc = p.gc().unwrap();
        assert_eq!(kinds(&gc), ["drop", "forget"]);
        assert!(gc.steps.contains(&Step::DropDatabase {
            name: "deleted_branch_db".into()
        }));
    }

    #[test]
    fn gc_keeps_fresh_branches_that_git_calls_merged() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "fresh"]);
        git(&repo, &["checkout", "-q", "-b", "done"]);
        let done_start = repo.resolve("HEAD").unwrap();
        commit_file(&repo, "x.txt", "work");
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["merge", "-q", "--ff-only", "done"]);

        let mut fresh = branch(2, "fresh", "fresh_db", &[], 2);
        fresh.base_commit = repo.resolve("fresh").unwrap();
        let mut done = branch(3, "done", "done_db", &[], 3);
        done.base_commit = done_start;
        let w = world(
            vec![branch(1, "main", "main_db", &[], 1), fresh, done],
            "main_db",
        );
        let c = cfg();
        let gc = Planner::new(&c, &repo, &w).gc().unwrap();
        assert_eq!(
            gc.steps,
            [
                Step::DropDatabase {
                    name: "done_db".into()
                },
                Step::ForgetBranch {
                    db_name: "done_db".into()
                }
            ]
        );
    }

    #[test]
    fn bench_stops_app_only_for_the_active_database() {
        let (_d, repo) = repo();
        let w = world(
            vec![
                branch(1, "main", "main_db", &[], 1),
                branch(2, "x", "x_db", &[], 2),
            ],
            "main_db",
        );
        let c = cfg();
        let p = Planner::new(&c, &repo, &w);
        assert_eq!(
            kinds(&p.bench(None).unwrap()),
            ["stop", "terminate", "clone", "start", "drop_temp"]
        );
        assert_eq!(kinds(&p.bench(Some("x")).unwrap()), ["clone", "drop_temp"]);
        assert!(!p.bench(None).unwrap().has_destructive());
    }

    #[test]
    fn disk_guard_refuses_and_warns() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "feature"]);
        let mut w = world(vec![branch(1, "main", "main_db", &[], 1)], "main_db");
        let c = cfg();
        w.disk = Some(DiskSpace {
            free_bytes: 500,
            total_bytes: 10_000,
        });
        let err = Planner::new(&c, &repo, &w).switch("feature").unwrap_err();
        assert!(format!("{err}").contains("not enough disk space"), "{err}");

        w.disk = Some(DiskSpace {
            free_bytes: 5_000,
            total_bytes: 10 << 30,
        });
        let plan = Planner::new(&c, &repo, &w).switch("feature").unwrap();
        assert!(
            plan.warnings.iter().any(|x| x.contains("leaves only")),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn rename_moves_the_database_and_the_env() {
        let (_d, repo) = repo();
        let w = world(
            vec![branch(1, "old/name", "old_name_db", &[], 1)],
            "old_name_db",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w)
            .rename("old/name", "new/name")
            .unwrap();
        assert_eq!(
            kinds(&plan),
            ["stop", "terminate", "rename", "env", "start", "relink"]
        );
        assert!(plan.steps.contains(&Step::RenameDatabase {
            from: "old_name_db".into(),
            to: "new_name_db".into()
        }));

        // Adopted databases keep their name; only the link moves.
        let mut adopted = branch(1, "main", "app_dev", &[], 1);
        adopted.adopted = true;
        let w = world(vec![adopted], "app_dev");
        let plan = Planner::new(&c, &repo, &w).rename("main", "trunk").unwrap();
        assert_eq!(kinds(&plan), ["relink"]);
    }

    #[test]
    fn gc_keeps_labelled_and_newest_snapshots() {
        let (_d, repo) = repo();
        let mut w = world(vec![branch(1, "main", "main_db", &[], 1)], "main_db");
        for (i, label) in [(3, None), (2, Some("keep-me")), (1, None), (0, None)] {
            let name = format!("main_db__s{i}");
            w.server_dbs.insert(
                name.clone(),
                DbInfo {
                    name: name.clone(),
                    size_bytes: 10,
                    allow_connections: false,
                },
            );
            w.snapshots.push(Snapshot {
                id: i,
                branch_id: 1,
                seq: i,
                label: label.map(String::from),
                db_name: name,
                commit_id: None,
                migration_files: FileSet::new(),
                size_bytes: Some(10),
                created_at: i,
            });
        }
        let mut c = cfg();
        c.snapshots.keep = Some(1);
        let gc = Planner::new(&c, &repo, &w).gc().unwrap();
        let dropped: Vec<&Step> = gc.steps.iter().filter(|s| s.is_destructive()).collect();
        assert_eq!(
            dropped,
            [
                &Step::DropDatabase {
                    name: "main_db__s1".into()
                },
                &Step::DropDatabase {
                    name: "main_db__s0".into()
                }
            ]
        );
    }

    /// With only records, a DB whose git branch moved on looks like it might hold
    /// the new migration, so an older branch can't use it. Its version table says
    /// otherwise.
    #[test]
    fn reading_lets_an_older_branch_reuse_a_db_behind_its_branch() {
        let (_d, repo) = repo();
        commit_file(&repo, "migrations/001.sql", "1");
        git(&repo, &["checkout", "-q", "-b", "old", "HEAD"]);
        git(&repo, &["checkout", "-q", "main"]);
        commit_file(&repo, "migrations/004.sql", "4");
        git(&repo, &["checkout", "-q", "old"]);

        let mut w = world(
            vec![branch(1, "main", "app_dev", &["001.sql"], 1)],
            "app_dev",
        );
        let c = cfg();
        let without = Planner::new(&c, &repo, &w).switch("old").unwrap();
        assert!(
            kinds(&without).contains(&"create_empty"),
            "{}",
            without.render(true)
        );

        w.readings.insert(
            "app_dev".into(),
            Reading {
                files: files(&["001.sql"]),
                unknown: vec![],
            },
        );
        w.version_table = Some("schema_migrations".into());
        let with = Planner::new(&c, &repo, &w).switch("old").unwrap();
        assert!(kinds(&with).contains(&"clone"), "{}", with.render(true));
        assert!(
            with.notes
                .iter()
                .any(|n| n.contains("per schema_migrations")),
            "{:?}",
            with.notes
        );
    }

    #[test]
    fn unknown_rows_rule_a_database_out_and_drive_warnings() {
        let (_d, repo) = repo();
        commit_file(&repo, "migrations/001.sql", "1");
        git(&repo, &["checkout", "-q", "-b", "feature"]);

        let mut w = world(
            vec![
                branch(1, "main", "main_db", &["001.sql"], 1),
                branch(2, "x", "x_db", &["001.sql"], 2),
            ],
            "main_db",
        );
        w.readings.insert(
            "main_db".into(),
            Reading {
                files: files(&["001.sql"]),
                unknown: vec![],
            },
        );
        // x_db's table has a row no branch knows (its branch was deleted).
        w.readings.insert(
            "x_db".into(),
            Reading {
                files: files(&["001.sql"]),
                unknown: vec!["999_gone".into()],
            },
        );
        let c = cfg();
        let p = Planner::new(&c, &repo, &w);
        let plan = p.switch("feature").unwrap();
        assert!(plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::CloneDatabase { source, .. } if source == "main_db")));

        // Switching to x warns about the unknown row, and nothing is pending.
        w.active_db = Some("main_db".into());
        let mut w2 = w.clone();
        w2.branches[1].git_branch = "feature".into();
        let plan = Planner::new(&c, &repo, &w2).switch("feature").unwrap();
        assert!(
            plan.warnings.iter().any(|x| x.contains("999_gone")),
            "{:?}",
            plan.warnings
        );
        assert!(!kinds(&plan).contains(&"migrate"));
        let status = Planner::new(&c, &repo, &w2)
            .migration_status("feature")
            .unwrap()
            .unwrap();
        assert_eq!(status.unknown, ["999_gone"]);
        assert!(status.pending.is_empty());
    }

    #[test]
    fn reconcile_reports_missing_databases() {
        let (_d, repo) = repo();
        let mut w = world(
            vec![
                branch(1, "main", "main_db", &[], 1),
                branch(2, "x", "x_db", &[], 2),
            ],
            "main_db",
        );
        w.server_dbs.remove("x_db");
        let c = cfg();
        let warnings = Planner::new(&c, &repo, &w).reconcile();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("x_db"));
    }
}
