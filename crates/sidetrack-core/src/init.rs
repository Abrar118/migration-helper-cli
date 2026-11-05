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

fn compose_path(root: &Path, file: &str) -> Result<std::path::PathBuf> {
    // Preserve symlink spelling: Compose resolves relative paths from the selected file.
    let mut path = std::path::PathBuf::new();
    for component in root.join(file).components() {
        match component {
            std::path::Component::ParentDir => {
                path.pop();
            }
            _ => path.push(component),
        }
    }
    anyhow::ensure!(
        path.is_file(),
        "the selected Compose file doesn't exist: {}",
        path.display()
    );
    Ok(path)
}

impl Setup {
    /// Read the repo at `dir`. Changes nothing.
    pub fn read(
        dir: &Path,
        main_branch: Option<String>,
        proxy: bool,
        compose_files: Vec<String>,
    ) -> Result<Self> {
        let git = Git::discover(dir)?;
        let root = git.root().to_path_buf();
        let compose_files: Vec<String> = compose_files
            .into_iter()
            .map(|file| {
                let path = compose_path(&root, &file)?;
                Ok(path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"))
            })
            .collect::<Result<_>>()?;
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
            if !compose_files.is_empty() {
                let saved: Vec<_> = cfg
                    .compose
                    .files
                    .iter()
                    .map(|file| compose_path(&root, file))
                    .collect::<Result<_>>()?;
                let selected: Vec<_> = compose_files
                    .iter()
                    .map(|file| compose_path(&root, file))
                    .collect::<Result<_>>()?;
                anyhow::ensure!(selected == saved,
                    "sidetrack.toml already selects the Compose files; edit [compose].files there before choosing different files");
            }
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

        let compose = Compose::new(
            &root,
            compose_files.iter().map(|file| root.join(file)).collect(),
        );
        let compose_dir = compose_files
            .first()
            .and_then(|file| Path::new(file).parent())
            .map(|dir| root.join(dir))
            .unwrap_or_else(|| root.clone());
        let views = ComposeViews {
            resolved: compose
                .config(ConfigView::Resolved)
                .context("reading the compose config")?,
            env_files: compose.config(ConfigView::EnvFiles)?,
            raw: compose.config(ConfigView::Raw)?,
        };
        let mut found = detect(&root, &compose_dir, &views)?;
        if found.user.is_none() {
            if let Some(file) = found.user_file.clone() {
                if let Ok(u) = compose.exec(&found.db_service, &["cat", &file]) {
                    found.user = Some(u.trim().to_string());
                }
            }
        }
        let cfg = found.to_config(main_branch, proxy, compose_files).context(
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

    pub fn proxy_upstream(&self) -> Result<Option<String>> {
        match &self.cfg.routing.upstream {
            Some(upstream) => Ok(Some(upstream.clone())),
            None => self.db_compose().port(&self.cfg.database.service, 5432),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn selected_parent_symlink_keeps_relative_bind_base() {
        if !docker_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("real/docker")).unwrap();
        std::os::unix::fs::symlink("real/docker", root.join("alias")).unwrap();
        std::fs::write(root.join("real/docker/dev.yml"), "name: sidetrack-parent-symlink-test\nservices:\n  db:\n    image: postgres:16\n    volumes: [\"../data:/var/lib/postgresql/data\"]\n").unwrap();
        let file = compose_path(&root, "alias/dev.yml").unwrap();
        let model = Compose::new(&root, vec![file])
            .config(ConfigView::Resolved)
            .unwrap();
        assert_eq!(
            model.services["db"].volumes[0].source.as_deref(),
            root.join("data").to_str()
        );
    }

    #[test]
    fn saved_proxy_setup_keeps_services_after_app_connection_changes() {
        if !docker_available() {
            return;
        }
        let (dir, _) = crate::git::testutil::repo();
        let root_path = dir.path().canonicalize().unwrap();
        let root = root_path.as_path();
        let compose = r#"name: sidetrack-proxy-setup-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=shop"]
  api:
    image: postgres:16
    environment: ["DATABASE_URL=postgresql://app@db:5432/shop"]
"#;
        std::fs::write(root.join("compose.yml"), compose).unwrap();
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = alembic\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
        let setup = Setup::read(root, None, true, vec![]).unwrap();
        assert_eq!(setup.cfg.routing.restart, ["api"]);
        setup.cfg.save(root).unwrap();
        std::fs::write(
            root.join("compose.yml"),
            compose.replace("@db:5432/", "@host.docker.internal:54320/"),
        )
        .unwrap();
        let reread = Setup::read(root, None, true, vec![]).unwrap();
        assert!(reread.detected.is_none());
        assert_eq!(reread.cfg, setup.cfg);
    }

    #[test]
    #[cfg(unix)]
    fn selected_compose_symlink_keeps_its_project_directory() {
        if !docker_available() {
            return;
        }
        let (dir, _) = crate::git::testutil::repo();
        let root_path = dir.path().canonicalize().unwrap();
        let root = root_path.as_path();
        std::fs::create_dir(root.join("docker")).unwrap();
        std::fs::write(
            root.join("docker/compose.yml"),
            r#"name: sidetrack-symlink-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=${POSTGRES_DB}"]
  api:
    image: postgres:16
    depends_on: [db]
    environment: ["DATABASE_URL=${DATABASE_URL}"]
"#,
        )
        .unwrap();
        std::os::unix::fs::symlink("docker/compose.yml", root.join("dev.yml")).unwrap();
        std::fs::write(
            root.join(".env"),
            "POSTGRES_DB=shop\nDATABASE_URL=postgresql://db/shop\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docker/.env"),
            "POSTGRES_DB=other\nDATABASE_URL=postgresql://db/other\n",
        )
        .unwrap();
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = alembic\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
        let setup = Setup::read(root, None, false, vec!["dev.yml".into()]).unwrap();
        assert_eq!(setup.cfg.compose.files, ["dev.yml"]);
        assert_eq!(setup.detected.unwrap().active_db, "shop");
        assert_eq!(setup.cfg.routing.file, ".env");
    }

    #[test]
    fn selected_nested_compose_uses_its_own_dotenv() {
        if !docker_available() {
            return;
        }
        let (dir, _) = crate::git::testutil::repo();
        let root_path = dir.path().canonicalize().unwrap();
        let root = root_path.as_path();
        std::fs::create_dir(root.join("docker")).unwrap();
        std::fs::write(
            root.join("docker/dev.yml"),
            r#"name: sidetrack-nested-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=shop"]
  api:
    image: postgres:16
    depends_on: [db]
    environment: ["DATABASE_URL=${DATABASE_URL}"]
"#,
        )
        .unwrap();
        std::fs::write(root.join(".env"), "DATABASE_URL=postgresql://db/wrong_db\n").unwrap();
        std::fs::write(
            root.join("docker/.env"),
            "DATABASE_URL=postgresql://db/shop\n",
        )
        .unwrap();
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = alembic\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
        let setup = Setup::read(root, None, false, vec!["docker/dev.yml".into()]).unwrap();
        assert_eq!(setup.cfg.routing.file, "docker/.env");
        assert_eq!(setup.cfg.routing.variable, "DATABASE_URL");
        assert_eq!(setup.cfg.compose.files, ["docker/dev.yml"]);
    }

    #[test]
    fn chosen_compose_files_are_used_and_persisted() {
        if !docker_available() {
            return;
        }
        let (dir, _) = crate::git::testutil::repo();
        let root_path = dir.path().canonicalize().unwrap();
        let root = root_path.as_path();
        std::fs::write(
            root.join("compose.yml"),
            "services:\n  unrelated:\n    image: redis:7\n",
        )
        .unwrap();
        std::fs::write(
            root.join("compose dev.yml"),
            r#"name: sidetrack-selection-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=shop", "POSTGRES_USER=app"]
  api:
    build: .
    depends_on: [db]
    environment: ["DATABASE_URL=${DATABASE_URL}"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".env"),
            "DATABASE_URL=postgresql://app:dummy@db:5432/shop\n",
        )
        .unwrap();
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = alembic\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
        assert!(Setup::read(root, None, false, vec![]).is_err());
        let setup = Setup::read(
            root,
            None,
            false,
            vec![root.join("compose dev.yml").display().to_string()],
        )
        .unwrap();
        assert_eq!(setup.cfg.compose.files, ["compose dev.yml"]);
        assert_eq!(setup.cfg.database.service, "db");
        assert!(setup
            .cfg
            .migrations
            .command
            .starts_with("docker compose -f 'compose dev.yml' exec -T api "));
        setup.cfg.save(root).unwrap();
        let reread = Setup::read(root, None, false, vec![]).unwrap();
        assert_eq!(reread.cfg, setup.cfg);
        let mut cfg = setup.cfg;
        cfg.compose.files = vec!["./compose dev.yml".into()];
        cfg.save(root).unwrap();
        assert_eq!(
            Setup::read(root, None, false, vec!["compose dev.yml".into()])
                .unwrap()
                .cfg,
            cfg
        );
        assert!(Setup::read(root, None, false, vec!["compose.yml".into()]).is_err());
    }
}
