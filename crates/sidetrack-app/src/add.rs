//! "Add project": what `sidetrack init` does, as two steps the user sees.
//! `preview` reads the repo and changes nothing; `add` reads it again, refuses
//! if the setup differs from the one shown, and registers the project.

use std::path::Path;

use anyhow::{bail, Result};
use serde::Serialize;
use sidetrack_core::config::Config;
use sidetrack_core::detect::Detection;
use sidetrack_core::hook;
use sidetrack_core::init::Setup;
use sidetrack_core::registry::{self, Registry};

use crate::views;

/// What adding the project at `root` would do.
#[derive(Debug, Serialize)]
pub struct AddPreview {
    /// The repo's top folder (the folder picked may be inside it).
    pub root: String,
    pub name: String,
    /// The checked-out branch, which gets linked to the database the app uses now.
    pub branch: Option<String>,
    /// Already in the registry: adding again only links what's new.
    pub registered: bool,
    /// What detection found; `None` when `sidetrack.toml` already exists.
    pub detected: Option<Detection>,
    pub config: Config,
    /// `sidetrack.toml` as it would be written (or as it is).
    pub toml: String,
    pub database_running: bool,
    pub hook_installed: bool,
    pub proxy_upstream: Option<String>,
    pub warnings: Vec<String>,
}

fn setup_warnings(setup: &Setup) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(warning) = setup.cfg.routing.network_warning() {
        warnings.push(warning.into());
    }
    if let Some(warning) = setup.cfg.compose.portability_warning() {
        warnings.push(warning.into());
    }
    if setup
        .detected
        .as_ref()
        .and_then(|d| d.migrations.command.as_ref())
        .is_some_and(|command| command != &setup.cfg.migrations.command)
    {
        warnings.push("The generated migration command now includes the selected Compose files, in merge order. Review the command below before adding.".into());
    }
    warnings
}

pub fn preview(dir: &str, compose_files: Vec<String>, proxy: bool) -> Result<AddPreview> {
    let setup = Setup::read(Path::new(dir), None, proxy, compose_files)?;
    let root = setup.git.root().to_string_lossy().into_owned();
    let key = setup
        .git
        .main_worktree()
        .ok()
        .flatten()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.clone());
    let registered = Registry::open(&registry::default_path()?)?
        .project_by_root(&key)?
        .is_some();
    Ok(AddPreview {
        name: setup
            .git
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone()),
        root,
        branch: setup.git.current_branch()?,
        registered,
        toml: setup.cfg.to_toml()?,
        database_running: setup.database_running()?,
        hook_installed: hook::is_installed(&setup.git),
        proxy_upstream: if setup.cfg.routing.is_proxy() {
            setup.proxy_upstream()?
        } else {
            None
        },
        warnings: setup_warnings(&setup),
        detected: setup.detected,
        config: setup.cfg,
    })
}

/// `docker compose up -d <database>` for the project at `root`.
pub fn start_database(root: &str, shown_toml: &str) -> Result<()> {
    reviewed_setup(root, shown_toml)?.start_database()
}

fn reviewed_setup(root: &str, shown_toml: &str) -> Result<Setup> {
    let shown: Config = toml::from_str(shown_toml)?;
    shown.validate()?;
    let setup = Setup::read(
        Path::new(root),
        None,
        shown.routing.is_proxy(),
        shown.compose.files,
    )?;
    if setup.cfg.to_toml()? != shown_toml {
        bail!("the project's setup changed since it was shown; review it again");
    }
    Ok(setup)
}

/// Register the project at `root`, if its setup still reads as `shown_toml`.
/// Returns what was done, as sentences.
pub fn add(root: &str, shown_toml: &str, install_hook: bool) -> Result<Vec<String>> {
    let setup = reviewed_setup(root, shown_toml)?;
    if setup.cfg.routing.is_proxy() && setup.proxy_upstream()?.is_none() {
        bail!("the database must publish port 5432 for the proxy; publish a local port in Compose or set routing.upstream in sidetrack.toml");
    }
    let (ws, mut said) = setup.register(Registry::open(&registry::default_path()?)?)?;
    if ws.cfg.routing.is_proxy() {
        if let Some(problem) = views::autostart(&ws) {
            said.push(format!("! {problem}"));
        }
        let port = ws
            .cfg
            .routing
            .listen_addr()
            .rsplit(':')
            .next()
            .unwrap_or("54320");
        said.push(format!("Once the proxy is running, point your app at host.docker.internal:{port} from containers or 127.0.0.1:{port} from this machine, keeping database {} and the existing credentials, JDBC prefix and URL options. Then recreate its containers using the selected Compose files.", ws.cfg.routing.database.as_deref().unwrap_or_default()));
    }
    if install_hook && !hook::is_installed(&ws.git) {
        // The hook runs `sidetrack` from PATH, else the CLI this app found.
        let exe = views::sidetrack_exe().unwrap_or_else(|| "sidetrack".into());
        said.push(format!("Hook {}.", hook::install(&ws.git, &exe)?));
        if views::sidetrack_exe().is_none() {
            said.push("! The hook needs the sidetrack CLI, which isn't installed here: `cargo install --path crates/sidetrack`.".into());
        }
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidetrack_core::compose::docker_available;

    #[test]
    fn fresh_setup_is_reviewed_without_writes_and_reports_command_changes() {
        if !docker_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        std::fs::write(
            root.join("dev.yml"),
            r#"name: sidetrack-fresh-review-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=shop"]
  api:
    build: .
    depends_on: [db]
    environment: ["DATABASE_URL=${DATABASE_URL}"]
"#,
        )
        .unwrap();
        std::fs::write(root.join(".env"), "DATABASE_URL=postgresql://db/shop\n").unwrap();
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = alembic\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
        let setup = Setup::read(&root, None, false, vec!["dev.yml".into()]).unwrap();
        let shown = setup.cfg.to_toml().unwrap();
        let mut reviewed = reviewed_setup(root.to_str().unwrap(), &shown).unwrap();
        assert!(reviewed.detected.is_some());
        assert!(setup_warnings(&reviewed)
            .iter()
            .any(|w| w.contains("migration command")));
        assert!(!root.join("sidetrack.toml").exists());
        reviewed.cfg.compose.files = vec![root.join("dev.yml").display().to_string()];
        assert!(setup_warnings(&reviewed)
            .iter()
            .any(|w| w.contains("absolute Compose paths")));
        reviewed.cfg.routing.use_proxy("shop");
        reviewed.cfg.routing.listen = Some("0.0.0.0:55432".into());
        assert!(setup_warnings(&reviewed)
            .iter()
            .any(|w| w.contains("Other machines on the network")));
        std::fs::write(
            root.join("alembic.ini"),
            "[alembic]\nscript_location = changed\n",
        )
        .unwrap();
        let error = match reviewed_setup(root.to_str().unwrap(), &shown) {
            Ok(_) => panic!("changed detection must require a new review"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("changed since it was shown"));
        assert!(!root.join("sidetrack.toml").exists());
    }

    #[test]
    fn reviewed_setup_keeps_selected_files_and_refuses_changed_settings() {
        if !docker_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success());
        std::fs::write(root.join("compose.yml"), "invalid: [").unwrap();
        std::fs::write(
            root.join("dev.yml"),
            "name: sidetrack-review-test\nservices:\n  db:\n    image: postgres:16\n",
        )
        .unwrap();
        let mut cfg: Config = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/e2e/fixture/sidetrack.toml"
        )))
        .unwrap();
        cfg.compose.files = vec!["dev.yml".into()];
        cfg.routing.use_proxy("app_dev");
        cfg.save(root).unwrap();
        let shown = cfg.to_toml().unwrap();
        let setup = reviewed_setup(root.to_str().unwrap(), &shown).unwrap();
        assert_eq!(setup.cfg.compose.files, ["dev.yml"]);
        assert!(setup.cfg.routing.is_proxy());
        cfg.migrations.command = "./changed.sh".into();
        cfg.save(root).unwrap();
        let error = match reviewed_setup(root.to_str().unwrap(), &shown) {
            Ok(_) => panic!("changed settings should require a new review"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("changed since it was shown"));
    }
}
