//! Prompts, approval and output helpers.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use anyhow::{bail, Result};
use sidetrack_core::apply::{Outcome, Report};
use sidetrack_core::plan::Plan;
use sidetrack_core::registry::{now, Project};
use sidetrack_core::workspace::Workspace;

#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    pub json: bool,
    pub yes: bool,
    pub dry_run: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Answer {
    Yes,
    No,
    Always,
}

/// Is there a terminal to ask on? Git GUIs run hooks without one.
pub fn has_tty() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .is_ok()
}

/// Ask on the terminal (not stdin, which git hooks don't connect).
pub fn ask(question: &str, allow_always: bool, default_yes: bool) -> Result<Answer> {
    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    let choices = match (allow_always, default_yes) {
        (true, _) => "[Y]es / [n]o / [a]lways for this repo",
        (false, true) => "[Y/n]",
        (false, false) => "[y/N]",
    };
    write!(tty, "{question} {choices} ")?;
    tty.flush()?;
    let mut line = String::new();
    BufReader::new(tty.try_clone()?).read_line(&mut line)?;
    Ok(match line.trim().to_lowercase().as_str() {
        "" if default_yes => Answer::Yes,
        "y" | "yes" => Answer::Yes,
        "a" | "always" if allow_always => Answer::Always,
        _ => Answer::No,
    })
}

/// Yes/no question for setup steps. `--yes` answers yes; no terminal means no.
pub fn confirm(question: &str, opts: &Opts) -> Result<bool> {
    if opts.yes {
        return Ok(true);
    }
    if !has_tty() {
        bail!("no terminal to ask \"{question}\"; re-run with --yes");
    }
    Ok(ask(question, false, true)? == Answer::Yes)
}

/// Show a plan, get approval, apply it. Returns false if it failed.
pub fn run_plan(ws: &Workspace, project: &Project, plan: &Plan, opts: &Opts) -> Result<bool> {
    if !opts.json {
        print!("{}", plan.render(opts.verbose));
    }
    if opts.dry_run {
        if opts.json {
            println!("{}", serde_json::json!({ "plan": plan, "applied": false }));
        }
        return Ok(true);
    }
    if plan.is_noop() {
        // Bookkeeping only (e.g. "mark as used"): no need to ask.
        let report = ws.applier(project).apply(plan, &mut |_| {})?;
        if opts.json {
            println!(
                "{}",
                serde_json::json!({ "plan": plan, "applied": true, "report": report })
            );
        }
        return Ok(report.ok());
    }

    let approved = if opts.yes {
        true
    } else if project.auto_approve && !plan.has_destructive() {
        if !opts.json {
            println!("Approved automatically (\"always\" is on for this repo).");
        }
        true
    } else if !has_tty() {
        bail!("no terminal to ask for approval; re-run with --yes (or --dry-run to only see the plan)");
    } else {
        let destructive = plan.has_destructive();
        let q = if destructive {
            "This deletes data. Apply?"
        } else {
            "Apply?"
        };
        match ask(q, !destructive, !destructive)? {
            Answer::Yes => true,
            Answer::Always => {
                ws.reg.set_auto_approve(project.id, true)?;
                println!(
                    "OK — future non-destructive plans in this repo will apply without asking."
                );
                true
            }
            Answer::No => false,
        }
    };
    if !approved {
        if opts.json {
            println!("{}", serde_json::json!({ "plan": plan, "applied": false }));
        } else {
            println!("Nothing changed.");
        }
        return Ok(true);
    }

    let report = ws.applier(project).apply(plan, &mut |line| {
        if !opts.json {
            eprintln!("{line}");
        }
    })?;
    if opts.json {
        println!(
            "{}",
            serde_json::json!({ "plan": plan, "applied": true, "report": report })
        );
    } else {
        print_outcome(&report);
    }
    Ok(report.ok())
}

pub fn print_outcome(report: &Report) {
    match &report.outcome {
        Outcome::Done => println!("Done."),
        Outcome::RolledBack {
            step,
            error,
            undo_errors,
        } => {
            println!("Failed at \"{step}\": {error}");
            if undo_errors.is_empty() {
                println!("Everything before it was undone; nothing changed.");
            } else {
                println!("Tried to undo earlier steps, but some undo steps failed:");
                for e in undo_errors {
                    println!("  - {e}");
                }
            }
        }
        Outcome::Stopped { step, error, note } => {
            println!("Failed at \"{step}\": {error}");
            println!("{note}");
        }
    }
}

pub fn ago(ts: i64) -> String {
    let d = (now() - ts).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}
