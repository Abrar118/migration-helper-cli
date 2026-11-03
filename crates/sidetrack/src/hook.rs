//! The git `post-checkout` hook (spec P0.10).
//!
//! Rules: the hook never fails or blocks a checkout, never prompts without a
//! terminal, and stays quiet when there's nothing to do.

use std::path::Path;

use anyhow::{bail, Result};
use sidetrack_core::config::Config;
use sidetrack_core::git::Git;
use sidetrack_core::workspace::Workspace;

use crate::ui::{self, Opts};

const BEGIN: &str = "# >>> sidetrack >>>";
const END: &str = "# <<< sidetrack <<<";

fn block() -> String {
    let fallback = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().replace('\'', ""))
        .unwrap_or_else(|| "sidetrack".into());
    format!(
        "{BEGIN}\n\
         # Switches the local database along with the git branch.\n\
         # Disable with `sidetrack hook uninstall`, or set SIDETRACK_SKIP=1 for one command.\n\
         st=\"$(command -v sidetrack || echo '{fallback}')\"\n\
         if [ -x \"$st\" ]; then \"$st\" hook run \"$1\" \"$2\" \"$3\" || true; fi\n\
         {END}\n"
    )
}

pub fn install(git: &Git) -> Result<String> {
    let dir = git.hooks_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("post-checkout");
    let existing = std::fs::read_to_string(&path).ok();
    let content = match existing {
        Some(c) if c.contains(BEGIN) => {
            return Ok(format!("already installed in {}", path.display()))
        }
        Some(c) => {
            let first = c.lines().next().unwrap_or("");
            if !(first.starts_with("#!") && (first.contains("sh") || first.contains("bash"))) {
                bail!(
                    "{} exists and isn't a shell script; add this line to it yourself:\n  sidetrack hook run \"$1\" \"$2\" \"$3\" || true",
                    path.display()
                );
            }
            // Insert right after the shebang so an `exit` later in the hook can't skip us.
            let rest = c.split_once('\n').map(|(_, r)| r).unwrap_or("");
            format!("{first}\n{}{rest}", block())
        }
        None => format!("#!/bin/sh\n{}", block()),
    };
    std::fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(format!("installed in {}", path.display()))
}

pub fn uninstall(git: &Git) -> Result<String> {
    let path = git.hooks_dir()?.join("post-checkout");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok("no hook installed".into());
    };
    let (Some(start), Some(end)) = (content.find(BEGIN), content.find(END)) else {
        return Ok("no sidetrack block in the hook".into());
    };
    let end = end + END.len() + usize::from(content[end + END.len()..].starts_with('\n'));
    let remaining = format!("{}{}", &content[..start], &content[end..]);
    let meaningful = remaining
        .lines()
        .any(|l| !l.trim().is_empty() && !l.starts_with("#!"));
    if meaningful {
        std::fs::write(&path, remaining)?;
    } else {
        std::fs::remove_file(&path)?;
    }
    Ok(format!("removed from {}", path.display()))
}

pub fn is_installed(git: &Git) -> bool {
    git.hooks_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("post-checkout")).ok())
        .is_some_and(|c| c.contains(BEGIN))
}

/// Entry point for the hook. Never returns an error: problems are printed.
pub fn run(dir: &Path, _old: &str, _new: &str, flag: &str) {
    if let Err(e) = run_inner(dir, flag) {
        eprintln!("sidetrack: {e:#}");
    }
}

fn run_inner(dir: &Path, flag: &str) -> Result<()> {
    // Note: we don't skip when old == new. `git checkout -b feature` keeps the same
    // commit but still needs its own database.
    if std::env::var("SIDETRACK_SKIP").is_ok_and(|v| !v.is_empty() && v != "0") || flag != "1" {
        return Ok(());
    }
    let Ok(git) = Git::discover(dir) else {
        return Ok(());
    };
    if !Config::exists(git.root()) || git.operation_in_progress()?.is_some() {
        return Ok(());
    }
    let ws = Workspace::open(git.root())?;
    let Some(branch) = git.current_branch()? else {
        let active = ws.active_db()?.unwrap_or_else(|| "?".into());
        eprintln!("sidetrack: detached HEAD; database left on {active}");
        return Ok(());
    };
    let Ok(project) = ws.project() else {
        eprintln!("sidetrack: run `sidetrack init` to link databases to branches in this repo");
        return Ok(());
    };
    let world = match ws.world() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("sidetrack: {e:#}; database left as is");
            return Ok(());
        }
    };
    let plan = ws.planner(&world).switch(&branch)?;
    if plan.is_noop() {
        // Already on the right database: just record it, silently.
        ws.applier(&project).apply(&plan, &mut |_| {})?;
        return Ok(());
    }
    let auto = project.auto_approve && !plan.has_destructive();
    if !auto && !ui::has_tty() {
        eprintln!(
            "sidetrack: {} — run `sidetrack switch` to apply",
            plan.summary
        );
        return Ok(());
    }
    let mut plan = plan;
    ws.annotate_schema(&mut plan);
    println!();
    let ok = ui::run_plan(&ws, &project, &plan, &Opts::default())?;
    if !ok {
        eprintln!("sidetrack: the git checkout itself is fine; only the database step failed");
    }
    Ok(())
}
