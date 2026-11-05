//! Setting a project up: read its compose setup (or its `sidetrack.toml`), then
//! register it and link the checked-out branch to the database it uses now.
//! Used by `sidetrack init` and the desktop app's "Add project".

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::compose::{docker_available, Compose, ConfigView};
use crate::config::{Config, CONFIG_FILE};
use crate::detect::{detect, ComposeViews, Detection};
use crate::git::Git;
use crate::registry::{NewBranch, Registry};
use crate::workspace::{compose_for, Workspace};
use crate::{migrations, naming};

/// A repo ready to register: its config, and what detection found when there
/// was no `sidetrack.toml` yet.
pub struct Setup {
    pub git: Git,
    pub cfg: Config,
    pub compose_project: String,
    /// `None` when `sidetrack.toml` already exists (it's used as is).
    pub detected: Option<Detection>,
}

impl Setup {
    /// Read the repo at `dir`. Changes nothing.
    pub fn read(dir: &Path, main_branch: Option<String>, proxy: bool) -> Result<Self> {
        let git = Git::discover(dir)?;
        let root = git.root().to_path_buf();
        anyhow::ensure!(
            docker_available(),
            "Docker isn't reachable; start Docker and try again"
        );
        let main_branch = match main_branch {
            Some(b) => Some(b),
            None => ["main", "master"]
                .into_iter()
                .find(|b| git.branch_exists(b).unwrap_or(false))
                .map(String::from),
        };

        if Config::exists(&root) {
            let cfg = Config::load(&root)?;
            if proxy && !cfg.routing.is_proxy() {
                bail!("{CONFIG_FILE} already exists in env mode; edit [routing] by hand (see the README's proxy section)");
            }
            let compose_project = compose_for(&cfg, &root).config(ConfigView::Resolved)?.name;
            return Ok(Self {
                git,
                cfg,
                compose_project,
                detected: None,
            });
        }

        let compose = Compose::new(&root, vec![]);
        let views = ComposeViews {
            resolved: compose
                .config(ConfigView::Resolved)
                .context("reading the compose config")?,
            env_files: compose.config(ConfigView::EnvFiles)?,
            raw: compose.config(ConfigView::Raw)?,
        };
        let mut found = detect(&root, &root, &views)?;
        if found.user.is_none() {
            if let Some(file) = found.user_file.clone() {
                if let Ok(u) = compose.exec(&found.db_service, &["cat", &file]) {
                    found.user = Some(u.trim().to_string());
                }
            }
        }
        let cfg = found.to_config(main_branch, proxy).context(
            "detection is incomplete; write sidetrack.toml by hand (see README) and run `sidetrack init` again",
        )?;
        Ok(Self {
            git,
            cfg,
            compose_project: views.resolved.name,
            detected: Some(found),
        })
    }

    /// The compose project that runs the database (the main worktree's).
    fn db_compose(&self) -> Compose {
        let main = self.git.main_worktree().ok().flatten();
        compose_for(&self.cfg, main.as_deref().unwrap_or(self.git.root()))
    }

    pub fn database_running(&self) -> Result<bool> {
        Ok(self
            .db_compose()
            .running_services()?
            .contains(&self.cfg.database.service))
    }

    /// `docker compose up -d <database service>`.
    pub fn start_database(&self) -> Result<()> {
        self.db_compose()
            .up(std::slice::from_ref(&self.cfg.database.service))
    }

    /// Register the project: write `sidetrack.toml` if it's new, link the
    /// checked-out branch to the database it uses now, and re-link databases
    /// sidetrack created earlier. Returns the workspace and what was done, as
    /// sentences. The database must be running; nothing is written if it isn't.
    pub fn register(self, reg: Registry) -> Result<(Workspace, Vec<String>)> {
        let Self {
            git,
            cfg,
            compose_project,
            detected,
        } = self;
        let root = git.root().to_path_buf();
        let ws = Workspace::from_parts(git, cfg, reg);
        anyhow::ensure!(
            ws.db_compose
                .running_services()?
                .contains(&ws.cfg.database.service),
            "start the database first: `docker compose up -d {}`",
            ws.cfg.database.service
        );
        let mut said = Vec::new();
        if detected.is_some() {
            ws.cfg.save(&root)?;
            said.push(format!(
                "Wrote {CONFIG_FILE}. It holds no secrets and is safe to commit."
            ));
        }
        let project = ws.reg.upsert_project(
            &ws.root_key(),
            &compose_project,
            &ws.cfg.database.engine,
            &ws.cfg.database.service,
        )?;

        // Adopt the current database for the current branch.
        let active = ws.active_db()?.with_context(|| {
            format!(
                "can't read a database name from {} in {}",
                ws.cfg.routing.variable, ws.cfg.routing.file
            )
        })?;
        anyhow::ensure!(
            ws.pg.exists(&active)?,
            "{active} (from {}) doesn't exist on the server",
            ws.cfg.routing.file
        );
        let branch = ws
            .git
            .current_branch()?
            .context("check out a branch before running init")?;
        let dirs = &ws.cfg.migrations.dirs;
        match (
            ws.reg.branch_by_db(project.id, &active)?,
            ws.reg.branch_by_git(project.id, &branch)?,
        ) {
            (None, None) => {
                ws.reg.insert_branch(
                    project.id,
                    &NewBranch {
                        git_branch: branch.clone(),
                        db_name: active.clone(),
                        parent_id: None,
                        base_commit: ws.git.resolve("HEAD")?,
                        migration_files: migrations::current_files(&ws.git, dirs)?,
                        adopted: true,
                    },
                )?;
                said.push(format!(
                    "Linked {branch} → {active} (assuming it has every migration in this checkout)."
                ));
            }
            (Some(b), _) => said.push(format!("{active} is already linked to {}.", b.git_branch)),
            (None, Some(b)) => said.push(format!("{branch} is already linked to {}.", b.db_name)),
        }

        // Re-link databases sidetrack created earlier (e.g. after the registry was deleted).
        for b in ws.git.local_branches()? {
            if ws.reg.branch_by_git(project.id, &b)?.is_some() {
                continue;
            }
            for name in [naming::db_name_for_branch(&b), naming::hashed_db_name(&b)] {
                if ws.reg.branch_by_db(project.id, &name)?.is_none() && ws.pg.exists(&name)? {
                    ws.reg.insert_branch(
                        project.id,
                        &NewBranch {
                            git_branch: b.clone(),
                            db_name: name.clone(),
                            parent_id: None,
                            base_commit: ws.git.resolve(&b)?,
                            migration_files: migrations::files_at(&ws.git, &b, dirs)?,
                            adopted: false,
                        },
                    )?;
                    said.push(format!("Re-linked {b} → {name}."));
                    break;
                }
            }
        }

        if !ws.cfg.routing.is_proxy() && ws.git.is_tracked(&ws.cfg.routing.file)? {
            said.push(format!(
                "! {} is tracked by git. sidetrack won't rewrite it; move {} into an untracked file first.",
                ws.cfg.routing.file, ws.cfg.routing.variable
            ));
        }
        Ok((ws, said))
    }
}
