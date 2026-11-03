//! One repo's sidetrack setup: config, git, compose, engine and registry together.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::apply::Applier;
use crate::compose::{docker_available, Compose};
use crate::config::Config;
use crate::engine::postgres::Postgres;
use crate::envfile;
use crate::git::Git;
use crate::planner::{Planner, World};
use crate::registry::{self, Project, Registry};

pub struct Workspace {
    pub root: PathBuf,
    pub git: Git,
    pub cfg: Config,
    pub compose: Compose,
    pub pg: Postgres,
    pub reg: Registry,
}

impl Workspace {
    /// Open the repo containing `dir`. Requires `sidetrack.toml`.
    pub fn open(dir: &Path) -> Result<Self> {
        let git = Git::discover(dir)?;
        let cfg = Config::load(git.root())?;
        let reg = Registry::open(&registry::default_path()?)?;
        Ok(Self::from_parts(git, cfg, reg))
    }

    pub fn from_parts(git: Git, cfg: Config, reg: Registry) -> Self {
        let root = git.root().to_path_buf();
        let compose = compose_for(&cfg, &root);
        let pg = postgres_for(&cfg, &compose);
        Self { root, git, cfg, compose, pg, reg }
    }

    pub fn root_key(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// The registered project, or a hint to run `init`.
    pub fn project(&self) -> Result<Project> {
        self.reg
            .project_by_root(&self.root_key())?
            .context("this repo isn't registered on this machine yet; run `sidetrack init`")
    }

    pub fn env_path(&self) -> PathBuf {
        self.root.join(&self.cfg.routing.file)
    }

    /// The database the app points at, read from the env file.
    pub fn active_db(&self) -> Result<Option<String>> {
        let path = self.env_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(envfile::get(&content, &self.cfg.routing.variable).and_then(|v| envfile::db_name_in_value(&v)))
    }

    /// Gather everything the planner needs. Needs Docker and the DB container.
    pub fn world(&self) -> Result<World> {
        anyhow::ensure!(docker_available(), "Docker isn't reachable; start Docker and try again");
        let project = self.project()?;
        let running_services = self.compose.running_services()?;
        anyhow::ensure!(
            running_services.contains(&self.cfg.database.service),
            "the {} service isn't running; start it with `docker compose up -d {}`",
            self.cfg.database.service,
            self.cfg.database.service
        );
        let server_dbs = self.pg.list_databases()?.into_iter().map(|d| (d.name.clone(), d)).collect();
        Ok(World {
            branches: self.reg.branches(project.id)?,
            snapshots: self.reg.snapshots(project.id)?,
            connections: self.pg.connections_by_db()?,
            active_db: self.active_db()?,
            env_file_tracked: self.git.is_tracked(&self.cfg.routing.file)?,
            server_dbs,
            running_services,
            project,
        })
    }

    pub fn planner<'a>(&'a self, world: &'a World) -> Planner<'a> {
        Planner::new(&self.cfg, &self.git, world)
    }

    pub fn applier<'a>(&'a self, project: &'a Project) -> Applier<'a> {
        Applier {
            cfg: &self.cfg,
            root: &self.root,
            compose: &self.compose,
            pg: &self.pg,
            reg: &self.reg,
            project,
        }
    }
}

pub fn compose_for(cfg: &Config, root: &Path) -> Compose {
    let files = cfg.compose.files.iter().map(|f| root.join(f)).collect();
    Compose::new(cfg.compose_dir(root), files)
}

pub fn postgres_for(cfg: &Config, compose: &Compose) -> Postgres {
    match &cfg.database.psql {
        Some(argv) => Postgres::new(argv.clone(), Some(compose.dir().to_path_buf())),
        None => Postgres::in_compose(compose, &cfg.database.service, &cfg.database.user),
    }
}
