//! One repo's sidetrack setup: config, git, compose, engine and registry together.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::apply::Applier;
use crate::compose::{docker_available, Compose};
use crate::config::Config;
use crate::engine::postgres::{DbInfo, Postgres};
use crate::envfile;
use crate::git::Git;
use crate::migrations;
use crate::plan::{Plan, PlanKind, Step};
use crate::planner::{DiskSpace, Planner, World};
use crate::readers::{self, Catalog, Reading};
use crate::registry::{self, DbBranch, Project, Registry};
use crate::schema;

pub struct Workspace {
    /// This checkout (a linked worktree's own root inside one).
    pub root: PathBuf,
    /// The main worktree's root. Equal to `root` outside linked worktrees. All
    /// worktrees share its database server and its registry entry.
    pub main_root: PathBuf,
    pub git: Git,
    pub cfg: Config,
    /// Compose in this checkout: the app services.
    pub compose: Compose,
    /// Compose in the main worktree: the database service.
    pub db_compose: Compose,
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
        let main_root = git.main_worktree().ok().flatten().unwrap_or(root.clone());
        let compose = compose_for(&cfg, &root);
        let db_compose = compose_for(&cfg, &main_root);
        let pg = postgres_for(&cfg, &db_compose);
        Self {
            root,
            main_root,
            git,
            cfg,
            compose,
            db_compose,
            pg,
            reg,
        }
    }

    /// Registry key: the main worktree's root, so worktrees share DB branches.
    pub fn root_key(&self) -> String {
        self.main_root.to_string_lossy().into_owned()
    }

    pub fn is_linked_worktree(&self) -> bool {
        self.main_root != self.root
    }

    /// The hold file, shared by every worktree's proxy.
    pub fn proxy_hold_path(&self) -> PathBuf {
        self.git
            .common_path("sidetrack-proxy-hold")
            .unwrap_or_else(|_| self.root.join(".git/sidetrack-proxy-hold"))
    }

    /// The address this checkout's proxy listens on. The main worktree uses
    /// `routing.listen`; each linked worktree gets the next free port after it,
    /// saved in its git dir so it never changes.
    pub fn proxy_listen(&self) -> String {
        let base = self.cfg.routing.listen_addr().to_string();
        if !self.is_linked_worktree() {
            return base;
        }
        let Ok(file) = self.git.git_path("sidetrack-proxy-listen") else {
            return base;
        };
        if let Ok(saved) = std::fs::read_to_string(&file) {
            if !saved.trim().is_empty() {
                return saved.trim().to_string();
            }
        }
        let Some((host, port)) = base.rsplit_once(':') else {
            return base;
        };
        let Ok(port) = port.parse::<u16>() else {
            return base;
        };
        let used: Vec<u16> = self
            .git
            .worktrees()
            .unwrap_or_default()
            .iter()
            .filter_map(|(path, _)| Git::discover(path).ok())
            .filter_map(|g| g.git_path("sidetrack-proxy-listen").ok())
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .filter_map(|s| s.trim().rsplit_once(':')?.1.parse().ok())
            .collect();
        let mine = (1..1000u16)
            .map(|k| port.saturating_add(k))
            .find(|p| !used.contains(p))
            .unwrap_or(port);
        let addr = format!("{host}:{mine}");
        let _ = std::fs::write(&file, format!("{addr}\n"));
        addr
    }

    /// Proxy mode in a linked worktree: this worktree's proxy port and what its
    /// env file says. `None` elsewhere, or when `routing.file` isn't set or is tracked.
    pub fn worktree_env(&self) -> Option<crate::planner::WorktreeEnv> {
        let r = &self.cfg.routing;
        if !r.is_proxy() || !self.is_linked_worktree() || r.file.is_empty() || r.variable.is_empty()
        {
            return None;
        }
        if self.git.is_tracked(&r.file).unwrap_or(true) {
            return None;
        }
        let port = self.proxy_listen().rsplit_once(':')?.1.parse().ok()?;
        let content = std::fs::read_to_string(self.env_path()).ok();
        let main_env = self.main_root.join(&r.file);
        Some(crate::planner::WorktreeEnv {
            port,
            env_exists: content.is_some(),
            env_port: content
                .as_deref()
                .and_then(|c| envfile::get(c, &r.variable))
                .and_then(|v| envfile::port_in_value(&v)),
            main_env: main_env.exists().then(|| main_env.display().to_string()),
        })
    }

    /// The database each *other* worktree's app uses: db name → worktree root.
    pub fn active_elsewhere(&self) -> BTreeMap<String, String> {
        let me = self.root.canonicalize().ok();
        let mut out = BTreeMap::new();
        for (path, _) in self.git.worktrees().unwrap_or_default() {
            if path.canonicalize().ok() == me {
                continue;
            }
            let Ok(g) = Git::discover(&path) else {
                continue;
            };
            let db = if self.cfg.routing.is_proxy() {
                let app_db = self.cfg.routing.database.clone().unwrap_or_default();
                g.git_path("sidetrack-proxy-target")
                    .ok()
                    .map(|t| crate::proxy::read_target(&t, &app_db))
            } else {
                std::fs::read_to_string(path.join(&self.cfg.routing.file))
                    .ok()
                    .and_then(|c| envfile::get(&c, &self.cfg.routing.variable))
                    .and_then(|v| envfile::db_name_in_value(&v))
            };
            if let Some(db) = db {
                out.insert(db, path.display().to_string());
            }
        }
        out
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

    /// Where `switch` records the proxy's target (inside `.git`, never committed).
    pub fn proxy_target_path(&self) -> PathBuf {
        self.git
            .git_path("sidetrack-proxy-target")
            .unwrap_or_else(|_| self.root.join(".git/sidetrack-proxy-target"))
    }

    /// Whether something accepts connections on the proxy's address.
    pub fn proxy_running(&self) -> bool {
        let addr = self.proxy_listen().replace("0.0.0.0", "127.0.0.1");
        addr.parse::<std::net::SocketAddr>().is_ok_and(|a| {
            std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(300)).is_ok()
        })
    }

    /// Proxy mode with `routing.autostart` on and nothing listening: commands
    /// should start the proxy before planning, so plans can hold connections
    /// instead of stopping the app.
    pub fn proxy_needs_start(&self) -> bool {
        let r = &self.cfg.routing;
        r.is_proxy() && r.autostart() && !self.proxy_running()
    }

    /// The background proxy's pid and log files, in this checkout's git dir.
    pub fn proxy_files(&self) -> Result<(PathBuf, PathBuf)> {
        Ok((
            self.git.git_path("sidetrack-proxy.pid")?,
            self.git.git_path("sidetrack-proxy.log")?,
        ))
    }

    /// Run `<exe> -C <root> proxy` in the background (`exe` is the sidetrack
    /// CLI) and wait until it accepts connections. Returns a line to show.
    pub fn start_proxy(&self, exe: &Path, listen: Option<&str>) -> Result<String> {
        let (pid_file, log_file) = self.proxy_files()?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .with_context(|| format!("opening {}", log_file.display()))?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("-C").arg(&self.root).arg("proxy");
        if let Some(l) = listen {
            cmd.args(["--listen", l]);
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        #[cfg(unix)]
        {
            // Its own process group, so Ctrl-C in the terminal (or the desktop
            // app quitting) doesn't stop it.
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().context("starting the proxy")?;
        let addr = listen
            .map(str::to_string)
            .unwrap_or_else(|| self.proxy_listen());
        let probe = addr.replace("0.0.0.0", "127.0.0.1");
        for _ in 0..50 {
            if let Some(status) = child.try_wait()? {
                let text = std::fs::read_to_string(&log_file).unwrap_or_default();
                let last = text.lines().last().unwrap_or("");
                bail!(
                    "the proxy stopped right away ({status}): {last}\nFull log: {}",
                    log_file.display()
                );
            }
            if std::net::TcpStream::connect(&probe).is_ok() {
                std::fs::write(&pid_file, format!("{}\n", child.id()))?;
                return Ok(format!(
                    "Proxy running on {addr} (pid {}). Log: {}. Stop it with `sidetrack proxy --stop`.",
                    child.id(),
                    log_file.display()
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = child.kill();
        bail!(
            "the proxy didn't start listening on {addr}; see {}",
            log_file.display()
        )
    }

    /// Postgres as reached from this machine: `routing.upstream`, else the port
    /// compose publishes for the database service.
    pub fn proxy_upstream(&self) -> Result<String> {
        if let Some(u) = &self.cfg.routing.upstream {
            return Ok(u.clone());
        }
        let port = self
            .db_compose
            .port(&self.cfg.database.service, 5432)?
            .with_context(|| {
                format!(
                    "{} doesn't publish port 5432; add `ports: [\"5432\"]` to it in compose, \
                 or set routing.upstream in sidetrack.toml",
                    self.cfg.database.service
                )
            })?;
        Ok(port
            .replace("0.0.0.0", "127.0.0.1")
            .replace("[::]", "127.0.0.1"))
    }

    /// The database the app points at: the proxy's target in proxy mode, else the
    /// name in the env file.
    pub fn active_db(&self) -> Result<Option<String>> {
        if self.cfg.routing.is_proxy() {
            let app_db = self.cfg.routing.database.clone().unwrap_or_default();
            return Ok(Some(crate::proxy::read_target(
                &self.proxy_target_path(),
                &app_db,
            )));
        }
        let path = self.env_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(envfile::get(&content, &self.cfg.routing.variable)
            .and_then(|v| envfile::db_name_in_value(&v)))
    }

    /// Gather everything the planner needs. Needs Docker and the DB container.
    pub fn world(&self) -> Result<World> {
        anyhow::ensure!(
            docker_available(),
            "Docker isn't reachable; start Docker and try again"
        );
        let project = self.project()?;
        let running_services = self.compose.running_services()?;
        let db_services = if self.is_linked_worktree() {
            self.db_compose.running_services()?
        } else {
            running_services.clone()
        };
        anyhow::ensure!(
            db_services.contains(&self.cfg.database.service),
            "the {} service isn't running; start it with `docker compose up -d {}`",
            self.cfg.database.service,
            self.cfg.database.service
        );
        let server_dbs: BTreeMap<String, DbInfo> = self
            .pg
            .list_databases()?
            .into_iter()
            .map(|d| (d.name.clone(), d))
            .collect();
        let branches = self.reg.branches(project.id)?;
        let (readings, version_table, reader_error) =
            match self.read_migrations(&branches, &server_dbs) {
                Ok(Some((table, r))) => (r, Some(table), None),
                Ok(None) => (Default::default(), None, None),
                Err(e) => (Default::default(), None, Some(format!("{e:#}"))),
            };
        Ok(World {
            branches,
            snapshots: self.reg.snapshots(project.id)?,
            connections: self.pg.connections_by_db()?,
            active_db: self.active_db()?,
            env_file_tracked: !self.cfg.routing.is_proxy()
                && self.git.is_tracked(&self.cfg.routing.file)?,
            proxy_running: self.cfg.routing.is_proxy().then(|| self.proxy_running()),
            elsewhere: self.active_elsewhere(),
            linked_worktree: self.is_linked_worktree(),
            worktree_env: self.worktree_env(),
            disk: self.disk_space(),
            readings,
            version_table,
            reader_error,
            server_dbs,
            running_services,
            project,
        })
    }

    /// Read each connectable DB branch's version table (one psql session) and map
    /// the rows to migration files. `None` when the tool has no version table.
    pub fn read_migrations(
        &self,
        branches: &[DbBranch],
        server_dbs: &BTreeMap<String, DbInfo>,
    ) -> Result<Option<(String, BTreeMap<String, Reading>)>> {
        let Some(reader) = readers::reader_for(&self.cfg.migrations) else {
            return Ok(None);
        };
        let dbs: Vec<String> = branches
            .iter()
            .filter(|b| {
                server_dbs
                    .get(&b.db_name)
                    .is_some_and(|d| d.allow_connections)
            })
            .map(|b| b.db_name.clone())
            .collect();
        let catalog = self.catalog(reader.kind == readers::Kind::AlembicHeads)?;
        let rows = self.pg.version_rows(&dbs, &reader.table, &reader.query)?;
        let readings = rows
            .into_iter()
            .map(|(db, rows)| {
                let reading = match rows {
                    Some(rows) => reader.interpret(&rows, &catalog),
                    None => Reading::default(), // no version table yet: nothing applied
                };
                (db, reading)
            })
            .collect();
        Ok(Some((reader.table, readings)))
    }

    /// Migration files across the working tree and every local branch, plus the
    /// Alembic revision chain when asked for.
    pub fn catalog(&self, alembic: bool) -> Result<Catalog> {
        let dirs = &self.cfg.migrations.dirs;
        // key → where to read it from (None = the working tree)
        let mut sources: BTreeMap<String, Option<String>> = BTreeMap::new();
        for key in migrations::files_in_worktree(&self.root, dirs)? {
            sources.insert(key, None);
        }
        let mut revs = vec!["HEAD".to_string()];
        revs.extend(self.git.local_branches()?);
        for rev in revs {
            for key in migrations::files_at(&self.git, &rev, dirs)? {
                sources.entry(key).or_insert_with(|| Some(rev.clone()));
            }
        }
        let mut catalog = Catalog {
            files: sources.keys().cloned().collect(),
            ..Default::default()
        };
        if alembic {
            let keys: Vec<&String> = sources.keys().filter(|k| k.ends_with(".py")).collect();
            let from_git: Vec<(usize, String)> = keys
                .iter()
                .enumerate()
                .filter_map(|(i, k)| sources[*k].as_ref().map(|rev| (i, format!("{rev}:{k}"))))
                .collect();
            let specs: Vec<String> = from_git.iter().map(|(_, s)| s.clone()).collect();
            let blobs = self.git.read_blobs(&specs)?;
            let mut contents: Vec<Option<String>> = vec![None; keys.len()];
            for ((i, _), blob) in from_git.into_iter().zip(blobs) {
                contents[i] = blob;
            }
            for (i, key) in keys.iter().enumerate() {
                let text = match &contents[i] {
                    Some(t) => Some(t.clone()),
                    None => std::fs::read_to_string(self.root.join(key)).ok(),
                };
                if let Some((rev, downs)) = text.as_deref().and_then(readers::parse_alembic) {
                    catalog.alembic.insert(rev, ((*key).clone(), downs));
                }
            }
        }
        Ok(catalog)
    }

    /// Free and total space where the database container keeps its data.
    /// Only known when psql runs inside the compose service (the default).
    pub fn disk_space(&self) -> Option<DiskSpace> {
        if self.cfg.database.psql.is_some() {
            return None;
        }
        let out = self
            .db_compose
            .exec(
                &self.cfg.database.service,
                &[
                    "sh",
                    "-c",
                    "df -Pk \"${PGDATA:-/var/lib/postgresql/data}\" | tail -n 1",
                ],
            )
            .ok()?;
        parse_df(&out)
    }

    /// For a switch between two existing databases, attach a summary of how the
    /// target's tables and columns differ from the current ones. Best effort:
    /// any error just leaves the plan without a summary.
    pub fn annotate_schema(&self, plan: &mut Plan) {
        let Some((from, to)) = schema_pair(plan) else {
            return;
        };
        if let (Ok(a), Ok(b)) = (self.pg.schema(&from), self.pg.schema(&to)) {
            plan.schema = Some(schema::diff(&from, &to, &a, &b));
        }
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
            proxy_target: self.proxy_target_path(),
            proxy_hold: self.proxy_hold_path(),
        }
    }
}

/// The two databases a switch plan moves the app between, when both already
/// exist: env mode rewrites the env file, proxy mode sets the proxy's target.
fn schema_pair(plan: &Plan) -> Option<(String, String)> {
    if plan.kind != PlanKind::Switch {
        return None;
    }
    let (from, to) = plan.steps.iter().find_map(|s| match s {
        Step::RewriteEnv { from_db, to_db, .. } | Step::SetProxyTarget { from_db, to_db, .. } => {
            Some((from_db.clone(), to_db.clone()))
        }
        _ => None,
    })?;
    let created_now = plan.steps.iter().any(|s| {
        matches!(s, Step::CloneDatabase { target, .. } | Step::CreateEmptyDatabase { name: target } if *target == to)
    });
    (!created_now).then_some((from, to))
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

/// Parse the last line of `df -Pk`: filesystem, 1K-blocks, used, available, …
fn parse_df(out: &str) -> Option<DiskSpace> {
    let line = out.lines().rev().find(|l| !l.trim().is_empty())?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = cols.get(1)?.parse().ok()?;
    let avail: u64 = cols.get(3)?.parse().ok()?;
    Some(DiskSpace {
        free_bytes: avail * 1024,
        total_bytes: total * 1024,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_df_output() {
        let out = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n\
                   overlay          263174212  61425604 188309384      25% /\n";
        let d = parse_df(out).unwrap();
        assert_eq!(d.total_bytes, 263174212 * 1024);
        assert_eq!(d.free_bytes, 188309384 * 1024);
        assert!(parse_df("garbage").is_none());
    }

    #[test]
    fn schema_pair_covers_both_routing_modes() {
        let pair = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        let mut env = Plan::new(PlanKind::Switch, "");
        env.push(Step::RewriteEnv {
            file: ".env".into(),
            variable: "DATABASE_URL".into(),
            from_db: "main_db".into(),
            to_db: "feat_db".into(),
        });
        assert_eq!(schema_pair(&env), pair("main_db", "feat_db"));

        let mut proxy = Plan::new(PlanKind::Switch, "");
        proxy.push(Step::SetProxyTarget {
            database: "app_dev".into(),
            from_db: "main_db".into(),
            to_db: "feat_db".into(),
        });
        assert_eq!(schema_pair(&proxy), pair("main_db", "feat_db"));

        // A database the plan creates has nothing to compare yet.
        proxy.steps.insert(
            0,
            Step::CloneDatabase {
                source: "main_db".into(),
                target: "feat_db".into(),
                size_bytes: 0,
            },
        );
        assert_eq!(schema_pair(&proxy), None);
    }
}
