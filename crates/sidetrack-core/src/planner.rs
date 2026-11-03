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

/// The source a new database is copied from.
struct Base {
    db_name: String,
    files: FileSet,
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
        self.world.server_dbs.get(db).map(|d| d.size_bytes).unwrap_or(0)
    }

    pub fn branch_for_git(&self, git_branch: &str) -> Option<&'a DbBranch> {
        self.world.branches.iter().find(|b| b.git_branch == git_branch)
    }

    pub fn branch_for_db(&self, db: &str) -> Option<&'a DbBranch> {
        self.world.branches.iter().find(|b| b.db_name == db)
    }

    fn branch_by_id(&self, id: i64) -> Option<&'a DbBranch> {
        self.world.branches.iter().find(|b| b.id == id)
    }

    fn active(&self) -> Result<&'a str> {
        self.world.active_db.as_deref().with_context(|| {
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
        if self.world.env_file_tracked {
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
                w.push(format!("snapshot {} is in the registry but not on the server", s.db_name));
            }
        }
        if let Some(active) = &self.world.active_db {
            if !self.live(active) {
                w.push(format!("the app points at {active}, which doesn't exist on the server"));
            } else if self.branch_for_db(active).is_none() {
                w.push(format!("the app points at {active}, which no git branch is linked to"));
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
    fn candidates(&self) -> Result<Vec<Candidate>> {
        let mut out = vec![];
        for b in &self.world.branches {
            if !self.live(&b.db_name) {
                continue;
            }
            // Over-count: what's recorded plus what its git branch now has.
            // A larger set only makes the candidate less likely to be picked.
            let mut files = b.migration_files.clone();
            if self.git.branch_exists(&b.git_branch)? {
                files.extend(migrations::files_at(self.git, &b.git_branch, &self.cfg.migrations.dirs)?);
            }
            out.push(Candidate {
                db_name: b.db_name.clone(),
                kind: CandidateKind::Branch,
                git_branch: b.git_branch.clone(),
                files,
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
            Base {
                db_name: c.db_name.clone(),
                files: c.files.clone(),
                parent_db,
                why: format!(
                    "{} is the closest compatible base ({what}, {} of {} migrations, none unknown to this branch)",
                    c.db_name,
                    c.files.len(),
                    target.len()
                ),
            }
        }))
    }

    /// `--from` accepts a git branch, a DB branch name or a snapshot name/label.
    fn explicit_base(&self, from: &str) -> Result<Base> {
        if let Some(b) = self.branch_for_git(from).or_else(|| self.branch_for_db(from)) {
            anyhow::ensure!(self.live(&b.db_name), "{} doesn't exist on the server", b.db_name);
            return Ok(Base {
                db_name: b.db_name.clone(),
                files: b.migration_files.clone(),
                parent_db: Some(b.db_name.clone()),
                why: format!("copying {} as requested", b.db_name),
            });
        }
        let s = self.find_snapshot(from)?;
        Ok(Base {
            db_name: s.db_name.clone(),
            files: s.migration_files.clone(),
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
            self.world.branches.iter().any(|b| b.db_name == name && b.git_branch != git_branch)
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
                let unknown = migrations::unknown(&target, &b.migration_files);
                if !unknown.is_empty() {
                    plan.warnings.push(format!(
                        "{} has {} migration(s) that {git_branch} doesn't have ({}); \
                         consider `sidetrack reset` if your migration tool complains",
                        b.db_name,
                        unknown.len(),
                        unknown.join(", ")
                    ));
                }
                let pending = migrations::pending(&target, &b.migration_files);
                if b.db_name == active {
                    plan.summary = format!("{git_branch} is already on {}", b.db_name);
                    if !pending.is_empty() {
                        self.push_migrations(&mut plan, &b.db_name, pending, &target);
                    }
                } else {
                    self.ensure_env_writable()?;
                    plan.summary = format!("Switch DB from {active} to {} for {git_branch}", b.db_name);
                    self.push_route(&mut plan, &active, &b.db_name, &self.running_app_services());
                    if !pending.is_empty() {
                        self.push_migrations(&mut plan, &b.db_name, pending, &target);
                    }
                }
                plan.push(Step::TouchBranch { db_name: b.db_name.clone() });
            }
            existing => {
                self.ensure_env_writable()?;
                if let Some(b) = existing {
                    plan.push(Step::ForgetBranch { db_name: b.db_name.clone() });
                }
                let name = match existing {
                    Some(b) if !self.live(&b.db_name) => b.db_name.clone(),
                    _ => self.choose_name(git_branch)?,
                };
                plan.summary = format!("Create {name} for {git_branch} and switch to it (from {active})");
                self.push_new_branch(&mut plan, git_branch, &name, &active, &target, None, false, true)?;
            }
        }
        Ok(plan)
    }

    /// Point the app at `to` and recreate the running app services.
    fn push_route(&self, plan: &mut Plan, from: &str, to: &str, services: &[String]) {
        plan.push(Step::RewriteEnv {
            file: self.cfg.routing.file.clone(),
            variable: self.cfg.routing.variable.clone(),
            from_db: from.to_string(),
            to_db: to.to_string(),
        });
        if services.is_empty() {
            plan.notes.push(format!(
                "none of {} is running; start them yourself to use the new database",
                self.cfg.routing.restart.join(", ")
            ));
        } else {
            plan.push(Step::StartServices { services: services.to_vec() });
        }
    }

    fn push_migrations(&self, plan: &mut Plan, db: &str, pending: Vec<String>, target: &FileSet) {
        if self.running_app_services().is_empty() && self.cfg.migrations.command.contains(" exec ") {
            plan.warnings.push(format!(
                "migrations run with `docker compose exec`, but {} isn't running",
                self.cfg.routing.restart.join(", ")
            ));
        }
        plan.push(Step::RunMigrations { command: self.cfg.migrations.command.clone(), pending });
        plan.push(Step::RecordMigrations { db_name: db.to_string(), migration_files: target.clone() });
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
        let base_commit = if self.git.branch_exists(git_branch)? { self.git.resolve(git_branch)? } else { None };
        let running = self.running_app_services();

        let applied = match &base {
            Some(base) => {
                plan.notes.push(base.why.clone());
                let stop = base.db_name == active && !running.is_empty();
                if stop {
                    plan.push(Step::StopServices { services: running.clone() });
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
                    migration_files: base.files.clone(),
                });
                if stop && !switch_to {
                    plan.push(Step::StartServices { services: running.clone() });
                }
                base.files.clone()
            }
            None => {
                if !empty {
                    plan.warnings.push(
                        "no existing database is compatible with this branch, so it will be built from empty"
                            .to_string(),
                    );
                }
                plan.push(Step::CreateEmptyDatabase { name: name.to_string() });
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
            self.push_route(plan, active, name, &running);
            let pending = migrations::pending(target, &applied);
            if !pending.is_empty() {
                self.push_migrations(plan, name, pending, target);
            }
            if base.is_none() {
                if let Some(seed) = &self.cfg.migrations.seed {
                    plan.push(Step::RunSeed { command: seed.clone() });
                }
            }
            plan.push(Step::TouchBranch { db_name: name.to_string() });
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
                plan.push(Step::TerminateConnections { db: db.to_string(), sessions: sessions.clone() });
                if app_stopped {
                    plan.notes.push("sessions from the stopped services close on their own".to_string());
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
            plan.push(Step::ForgetBranch { db_name: b.db_name.clone() });
        }
        self.push_new_branch(&mut plan, git_branch, &name, &active, &target, from, empty, false)?;
        Ok(plan)
    }

    // ---------------------------------------------------------------- snapshot

    fn active_branch(&self) -> Result<&'a DbBranch> {
        let active = self.active()?;
        self.branch_for_db(active)
            .with_context(|| format!("{active} isn't linked to a git branch; run `sidetrack init` or `sidetrack switch`"))
    }

    pub fn snapshot(&self, label: Option<&str>, next_seq: i64) -> Result<Plan> {
        let b = self.active_branch()?;
        if let Some(l) = label {
            anyhow::ensure!(
                self.world.snapshots.iter().all(|s| s.label.as_deref() != Some(l)),
                "a snapshot labelled {l:?} already exists"
            );
        }
        let name = naming::snapshot_db_name(&b.db_name, next_seq);
        let mut plan = Plan::new(PlanKind::Snapshot, format!("Snapshot {} as {name}", b.db_name));
        plan.warnings = self.reconcile();
        let running = self.running_app_services();
        if !running.is_empty() {
            plan.push(Step::StopServices { services: running.clone() });
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
            migration_files: b.migration_files.clone(),
            size_bytes: self.size(&b.db_name),
        });
        if !running.is_empty() {
            plan.push(Step::StartServices { services: running });
        }
        Ok(plan)
    }

    pub fn snapshot_remove(&self, name: &str) -> Result<Plan> {
        let s = self.find_snapshot(name)?;
        let mut plan = Plan::new(PlanKind::SnapshotRemove, format!("Delete snapshot {}", s.db_name));
        if self.live(&s.db_name) {
            plan.push(Step::DropDatabase { name: s.db_name.clone() });
        }
        plan.push(Step::ForgetSnapshot { db_name: s.db_name.clone() });
        Ok(plan)
    }

    // ------------------------------------------------------------------- reset

    pub fn reset(&self, to: &ResetTo) -> Result<Plan> {
        let b = self.active_branch()?;
        let own_latest = || self.world.snapshots.iter().find(|s| s.branch_id == b.id && self.live(&s.db_name));
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
                let p = parent().with_context(|| format!("{} has no parent DB branch on the server", b.db_name))?;
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
        let mut plan = Plan::new(PlanKind::Reset, format!("Reset {} to a fresh copy of {source}", b.db_name));
        plan.warnings = self.reconcile();
        if self.live(&tmp) {
            plan.warnings.push(format!("{tmp} is left over from an earlier reset and will be replaced"));
            plan.push(Step::DropDatabase { name: tmp.clone() });
        }
        let running = self.running_app_services();
        if !running.is_empty() {
            plan.push(Step::StopServices { services: running.clone() });
        }
        self.push_terminate(&mut plan, &source, false);
        plan.push(Step::CloneDatabase { source: source.clone(), target: tmp.clone(), size_bytes: self.size(&source) });
        self.push_terminate(&mut plan, &b.db_name, !running.is_empty());
        plan.push(Step::DropDatabase { name: b.db_name.clone() });
        plan.push(Step::RenameDatabase { from: tmp, to: b.db_name.clone() });
        plan.push(Step::RecordMigrations { db_name: b.db_name.clone(), migration_files: files.clone() });
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
        plan.push(Step::TouchBranch { db_name: b.db_name.clone() });
        Ok(plan)
    }

    // ------------------------------------------------------------------ delete

    pub fn delete(&self, name: &str) -> Result<Plan> {
        let b = self
            .branch_for_git(name)
            .or_else(|| self.branch_for_db(name))
            .with_context(|| format!("no DB branch for {name}"))?;
        if self.world.active_db.as_deref() == Some(b.db_name.as_str()) {
            bail!("{} is the active database; switch to another branch first", b.db_name);
        }
        let mut plan = Plan::new(PlanKind::Delete, format!("Delete {} (git branch {})", b.db_name, b.git_branch));
        if b.adopted {
            plan.warnings.push(format!("{} existed before sidetrack (it was adopted at init)", b.db_name));
        }
        self.push_delete_branch(&mut plan, b);
        Ok(plan)
    }

    fn push_delete_branch(&self, plan: &mut Plan, b: &DbBranch) {
        for s in self.world.snapshots.iter().filter(|s| s.branch_id == b.id) {
            if self.live(&s.db_name) {
                plan.push(Step::DropDatabase { name: s.db_name.clone() });
            }
            plan.push(Step::ForgetSnapshot { db_name: s.db_name.clone() });
        }
        if self.live(&b.db_name) {
            plan.push(Step::DropDatabase { name: b.db_name.clone() });
        }
        plan.push(Step::ForgetBranch { db_name: b.db_name.clone() });
    }

    // ---------------------------------------------------------------------- gc

    pub fn gc(&self) -> Result<Plan> {
        let mut plan = Plan::new(PlanKind::Gc, "Clean up DB branches whose git branch is gone or merged".to_string());
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
            let is_merged = merged.contains(&b.git_branch) && !b.adopted;
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
            plan.notes.push(format!("{} ({}): {why}", b.db_name, b.git_branch));
            freed += self.size(&b.db_name);
            for s in self.world.snapshots.iter().filter(|s| s.branch_id == b.id) {
                freed += self.size(&s.db_name);
            }
            self.push_delete_branch(&mut plan, b);
        }
        for s in &self.world.snapshots {
            if !self.live(&s.db_name) && !plan.steps.iter().any(|st| matches!(st, Step::ForgetSnapshot { db_name } if *db_name == s.db_name)) {
                plan.push(Step::ForgetSnapshot { db_name: s.db_name.clone() });
            }
        }
        if freed > 0 {
            plan.notes.push(format!("frees about {}", crate::engine::postgres::format_bytes(freed)));
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ComposeConfig, DatabaseConfig, GitConfig, MigrationsConfig, RoutingConfig};
    use crate::git::testutil::*;

    fn cfg() -> Config {
        Config {
            database: DatabaseConfig { engine: "postgres".into(), service: "db".into(), user: "postgres".into(), psql: None },
            routing: RoutingConfig {
                mode: "env".into(),
                file: ".env".into(),
                variable: "DATABASE_URL".into(),
                restart: vec!["api".into()],
            },
            migrations: MigrationsConfig {
                tool: "generic".into(),
                dirs: vec!["migrations".into()],
                command: "./migrate".into(),
                seed: Some("./seed".into()),
            },
            compose: ComposeConfig::default(),
            git: GitConfig { main_branch: Some("main".into()) },
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
            .map(|b| (b.db_name.clone(), DbInfo { name: b.db_name.clone(), size_bytes: 1000, allow_connections: true }))
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
                Step::StartServices { .. } => "start",
                Step::RunMigrations { .. } => "migrate",
                Step::RunSeed { .. } => "seed",
                Step::RegisterBranch { .. } => "register",
                Step::RecordMigrations { .. } => "record",
                Step::TouchBranch { .. } => "touch",
                Step::RegisterSnapshot { .. } => "register_snapshot",
                Step::ForgetBranch { .. } => "forget",
                Step::ForgetSnapshot { .. } => "forget_snapshot",
            })
            .collect()
    }

    /// Spec §1: A has 2 migrations, B adds 4 more. Returning to A just switches back.
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
                branch(2, "b", "b_db", &["001.sql", "002.sql", "003.sql", "004.sql", "005.sql", "006.sql"], 2),
            ],
            "b_db",
        );
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("a").unwrap();
        assert_eq!(kinds(&plan), ["env", "start", "touch"], "{}", plan.render(true));
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
            ["stop", "terminate", "clone", "register", "env", "start", "migrate", "record", "touch"],
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

    #[test]
    fn no_compatible_base_builds_from_empty_and_seeds() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "other"]);
        commit_file(&repo, "migrations/009.sql", "9");
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["checkout", "-q", "-b", "fresh"]);
        commit_file(&repo, "migrations/001.sql", "1");

        let w = world(vec![branch(1, "other", "other_db", &["009.sql"], 1)], "other_db");
        let c = cfg();
        let plan = Planner::new(&c, &repo, &w).switch("fresh").unwrap();
        assert_eq!(kinds(&plan), ["create_empty", "register", "env", "start", "migrate", "record", "seed", "touch"]);
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn name_collision_uses_hash() {
        let (_d, repo) = repo();
        git(&repo, &["checkout", "-q", "-b", "feature-foo"]);
        let w = world(vec![branch(1, "feature/Foo", "feature_foo_db", &[], 1)], "feature_foo_db");
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
        assert_eq!(kinds(&plan), ["stop", "clone", "terminate", "drop", "rename", "record", "start", "touch"]);
        assert!(plan.has_destructive());
    }

    #[test]
    fn delete_refuses_active_and_gc_finds_gone_branches() {
        let (_d, repo) = repo();
        let w = world(
            vec![branch(1, "main", "main_db", &[], 1), branch(2, "deleted/branch", "deleted_branch_db", &[], 2)],
            "main_db",
        );
        let c = cfg();
        let p = Planner::new(&c, &repo, &w);
        assert!(p.delete("main").is_err());
        let gc = p.gc().unwrap();
        assert_eq!(kinds(&gc), ["drop", "forget"]);
        assert!(gc.steps.contains(&Step::DropDatabase { name: "deleted_branch_db".into() }));
    }

    #[test]
    fn reconcile_reports_missing_databases() {
        let (_d, repo) = repo();
        let mut w = world(vec![branch(1, "main", "main_db", &[], 1), branch(2, "x", "x_db", &[], 2)], "main_db");
        w.server_dbs.remove("x_db");
        let c = cfg();
        let warnings = Planner::new(&c, &repo, &w).reconcile();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("x_db"));
    }
}
