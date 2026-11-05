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
}

pub fn preview(dir: &str) -> Result<AddPreview> {
    let setup = Setup::read(Path::new(dir), None, false)?;
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
        detected: setup.detected,
        config: setup.cfg,
    })
}

/// `docker compose up -d <database>` for the project at `root`.
pub fn start_database(root: &str) -> Result<()> {
    Setup::read(Path::new(root), None, false)?.start_database()
}

/// Register the project at `root`, if its setup still reads as `shown_toml`.
/// Returns what was done, as sentences.
pub fn add(root: &str, shown_toml: &str, install_hook: bool) -> Result<Vec<String>> {
    let setup = Setup::read(Path::new(root), None, false)?;
    if setup.cfg.to_toml()? != shown_toml {
        bail!("the project's setup changed since it was shown; review it again");
    }
    let (ws, mut said) = setup.register(Registry::open(&registry::default_path()?)?)?;
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
