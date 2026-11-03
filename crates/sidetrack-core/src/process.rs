//! Small helpers for running external commands with useful error messages.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Run and capture stdout. Fails with stderr in the message on a non-zero exit.
pub fn capture(cmd: &mut Command) -> Result<String> {
    let shown = display(cmd);
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not start `{shown}`"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        bail!("`{shown}` failed ({}): {detail}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Like `capture`, feeding `input` on stdin.
pub fn capture_with_input(cmd: &mut Command, input: &str) -> Result<String> {
    use std::io::Write;
    let shown = display(cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start `{shown}`"))?;
    let mut stdin = child.stdin.take().context("stdin")?;
    let input = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("stdin writer panicked"))??;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("`{shown}` failed ({}): {}", out.status, stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run and capture stdout, or `None` on a non-zero exit.
pub fn capture_opt(cmd: &mut Command) -> Result<Option<String>> {
    let shown = display(cmd);
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("could not start `{shown}`"))?;
    Ok(out
        .status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Run a user-supplied shell command (e.g. the migration command). Its output goes
/// to our stderr so machine-readable stdout (`--json`) stays clean.
pub fn shell(command: &str, dir: &Path) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .with_context(|| format!("could not start `{command}`"))?;
    if !status.success() {
        bail!("`{command}` failed ({status})");
    }
    Ok(())
}

/// Run with inherited stderr (progress output), capturing nothing.
pub fn run(cmd: &mut Command) -> Result<()> {
    capture(cmd).map(|_| ())
}

pub fn display(cmd: &Command) -> String {
    let mut parts = vec![cmd.get_program().to_string_lossy().into_owned()];
    parts.extend(cmd.get_args().map(|a| {
        let a = a.to_string_lossy();
        if a.contains(' ') {
            format!("'{a}'")
        } else {
            a.into_owned()
        }
    }));
    parts.join(" ")
}
