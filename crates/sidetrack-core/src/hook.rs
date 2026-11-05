//! Installing the git `post-checkout` hook (spec P0.10). Running it is the
//! CLI's job (`sidetrack hook run`); this only writes and removes the block.

use std::path::Path;

use anyhow::{bail, Result};

use crate::git::Git;

const BEGIN: &str = "# >>> sidetrack >>>";
const END: &str = "# <<< sidetrack <<<";

/// The hook block. It runs `sidetrack` from PATH, or `exe` when it isn't there.
fn block(exe: &Path) -> String {
    let fallback = exe.to_string_lossy().replace('\'', "");
    format!(
        "{BEGIN}\n\
         # Switches the local database along with the git branch.\n\
         # Disable with `sidetrack hook uninstall`, or set SIDETRACK_SKIP=1 for one command.\n\
         st=\"$(command -v sidetrack || echo '{fallback}')\"\n\
         if [ -x \"$st\" ]; then \"$st\" hook run \"$1\" \"$2\" \"$3\" || true; fi\n\
         {END}\n"
    )
}

/// Add the block to `post-checkout`, keeping any hook that's already there.
/// `exe` is the sidetrack CLI to fall back on when it isn't on PATH.
pub fn install(git: &Git, exe: &Path) -> Result<String> {
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
            format!("{first}\n{}{rest}", block(exe))
        }
        None => format!("#!/bin/sh\n{}", block(exe)),
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
