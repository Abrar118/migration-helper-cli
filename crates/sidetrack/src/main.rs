//! `sidetrack` — give every git branch its own local database.

mod hook;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use sidetrack_core::compose::docker_available;
use sidetrack_core::config::Config;
use sidetrack_core::engine::postgres::format_bytes;
use sidetrack_core::git::Git;
use sidetrack_core::init::Setup;
use sidetrack_core::naming;
use sidetrack_core::planner::ResetTo;
use sidetrack_core::registry::{self, Registry};
use sidetrack_core::workspace::Workspace;

use crate::ui::{ago, confirm, run_plan, Opts};
use sidetrack_core::apply::Report;
use sidetrack_core::plan::{Plan, Step};

#[derive(Parser)]
#[command(
    name = "sidetrack",
    version,
    about = "Give every git branch its own local database."
)]
struct Cli {
    /// Run as if started in this directory.
    #[arg(short = 'C', global = true, value_name = "DIR")]
    dir: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    /// Apply without asking.
    #[arg(long, short = 'y', global = true)]
    yes: bool,
    /// Show the plan and stop.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Also show registry bookkeeping steps.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect your setup, write sidetrack.toml and link the current branch to the current database.
    Init {
        /// Branch that features merge into (used by `gc`). Detected if omitted.
        #[arg(long)]
        main_branch: Option<String>,
        /// Don't offer to install the git hook.
        #[arg(long)]
        no_hook: bool,
        /// Route the app through `sidetrack proxy` instead of rewriting its env file.
        #[arg(long)]
        proxy: bool,
    },
    /// Show the current branch, the active database and whether they match.
    Status,
    /// List DB branches and snapshots.
    List,
    /// Switch the database to the one for BRANCH (default: the checked-out branch).
    Switch { branch: Option<String> },
    /// Create a DB branch without switching to it.
    Create {
        branch: String,
        /// Copy this DB branch, git branch or snapshot instead of picking automatically.
        #[arg(long, conflicts_with = "empty")]
        from: Option<String>,
        /// Start from an empty database.
        #[arg(long)]
        empty: bool,
    },
    /// Snapshot the active database (or list / remove snapshots).
    #[command(args_conflicts_with_subcommands = true)]
    Snapshot {
        #[command(subcommand)]
        action: Option<SnapshotAction>,
        /// Optional label for the new snapshot.
        label: Option<String>,
    },
    /// Replace the active database with a fresh copy of a snapshot or its parent.
    Reset {
        /// Snapshot name or label.
        #[arg(long, conflicts_with = "parent")]
        to: Option<String>,
        /// Copy the parent DB branch instead.
        #[arg(long)]
        parent: bool,
    },
    /// Drop a DB branch and its snapshots.
    Delete { branch: String },
    /// Offer to delete DB branches whose git branch is deleted or merged.
    Gc,
    /// Manage the git post-checkout hook.
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Check that everything sidetrack needs is in place.
    Doctor,
    /// Time how long copying a database takes here, then delete the copy.
    Bench {
        /// Database, DB branch or snapshot to copy (default: the active database).
        #[arg(long)]
        from: Option<String>,
    },
    /// Follow a git branch rename: move OLD's DB branch to NEW.
    Rename { old: String, new: String },
    /// Run the local Postgres proxy (routing.mode = "proxy"). Stays in the foreground.
    Proxy {
        /// Address to listen on (default: routing.listen, else 127.0.0.1:54320).
        #[arg(long)]
        listen: Option<String>,
        /// Run in the background (log and pid file in .git/).
        #[arg(long, conflicts_with = "stop")]
        daemon: bool,
        /// Stop a proxy started with --daemon.
        #[arg(long)]
        stop: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// List snapshots.
    List,
    /// Delete a snapshot.
    Rm { name: String },
}

#[derive(Subcommand)]
enum HookAction {
    Install,
    Uninstall,
    /// Called by git; not for direct use.
    #[command(hide = true)]
    Run {
        old: String,
        new: String,
        flag: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let dir = cli.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let opts = Opts {
        json: cli.json,
        yes: cli.yes,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
    };

    if let Command::Hook {
        action: HookAction::Run { old, new, flag },
    } = &cli.command
    {
        hook::run(&dir, old, new, flag);
        return ExitCode::SUCCESS; // never fail a checkout
    }
    match dispatch(cli.command, &dir, &opts) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            if opts.json {
                println!("{}", json!({ "error": format!("{e:#}") }));
            } else {
                eprintln!("error: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(command: Command, dir: &Path, opts: &Opts) -> Result<bool> {
    match command {
        Command::Init {
            main_branch,
            no_hook,
            proxy,
        } => init(dir, opts, main_branch, no_hook, proxy),
        Command::Status => status(dir, opts),
        Command::List => list(dir, opts),
        Command::Doctor => doctor(dir, opts),
        Command::Proxy {
            listen,
            daemon,
            stop,
        } => match (daemon, stop) {
            (_, true) => proxy_stop(dir),
            (true, _) => proxy_daemon(dir, listen),
            _ => proxy(dir, listen),
        },
        Command::Hook { action } => {
            let git = Git::discover(dir)?;
            let msg = match action {
                HookAction::Install => hook::install(&git)?,
                HookAction::Uninstall => hook::uninstall(&git)?,
                HookAction::Run { .. } => unreachable!("handled in main"),
            };
            println!("Hook {msg}.");
            Ok(true)
        }
        Command::Snapshot {
            action: Some(SnapshotAction::List),
            ..
        } => snapshot_list(dir, opts),
        command => plan_command(command, dir, opts),
    }
}

/// Commands that build a plan and apply it.
fn plan_command(command: Command, dir: &Path, opts: &Opts) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    if !opts.dry_run {
        autostart_proxy(&ws);
    }
    let world = ws.world()?;
    let planner = ws.planner(&world);
    let plan = match command {
        Command::Switch { branch } => {
            let current = ws.git.current_branch()?;
            let branch = match (branch, &current) {
                (Some(b), _) => b,
                (None, Some(c)) => c.clone(),
                (None, None) => {
                    bail!("HEAD is detached; name the branch: `sidetrack switch <branch>`")
                }
            };
            let mut plan = planner.switch(&branch)?;
            ws.annotate_schema(&mut plan);
            if current.as_deref() != Some(branch.as_str()) {
                plan.warnings.push(format!(
                    "you have {} checked out, so the code won't match {branch}'s database",
                    current.as_deref().unwrap_or("a detached HEAD")
                ));
            }
            plan
        }
        Command::Create {
            branch,
            from,
            empty,
        } => planner.create(&branch, from.as_deref(), empty)?,
        Command::Snapshot {
            action: Some(SnapshotAction::Rm { name }),
            ..
        } => planner.snapshot_remove(&name)?,
        Command::Snapshot { label, .. } => {
            let active = world
                .active_db
                .as_deref()
                .context("can't read the active database")?;
            let branch = planner
                .branch_for_db(active)
                .with_context(|| format!("{active} isn't linked to a git branch"))?;
            let seq = ws.reg.next_snapshot_seq(branch.id)?;
            planner.snapshot(label.as_deref(), seq)?
        }
        Command::Reset { to, parent } => {
            let target = match (to, parent) {
                (Some(n), _) => ResetTo::Snapshot(n),
                (None, true) => ResetTo::Parent,
                (None, false) => ResetTo::Latest,
            };
            planner.reset(&target)?
        }
        Command::Delete { branch } => planner.delete(&branch)?,
        Command::Gc => planner.gc()?,
        Command::Rename { old, new } => planner.rename(&old, &new)?,
        Command::Bench { from } => {
            let plan = planner.bench(from.as_deref())?;
            return ui::run_plan_then(&ws, &world.project, &plan, opts, |report| {
                bench_summary(&plan, report)
            });
        }
        _ => unreachable!("not a plan command"),
    };
    run_plan(&ws, &world.project, &plan, opts)
}

// ------------------------------------------------------------------------ init

fn init(
    dir: &Path,
    opts: &Opts,
    main_branch: Option<String>,
    no_hook: bool,
    proxy: bool,
) -> Result<bool> {
    let setup = Setup::read(dir, main_branch, proxy)?;
    match &setup.detected {
        None => println!(
            "Using the existing {}.",
            sidetrack_core::config::CONFIG_FILE
        ),
        Some(found) => {
            let cfg = &setup.cfg;
            if opts.json {
                println!("{}", json!({ "detected": found }));
            } else {
                println!(
                    "Detected:
{}",
                    found.render(proxy)
                );
            }
            if proxy {
                let url = if cfg.routing.file.is_empty() {
                    let vars: Vec<String> = found
                        .inline
                        .iter()
                        .map(|i| format!("{} ({})", i.variable, i.service))
                        .collect();
                    format!("{} in the compose file", vars.join(", "))
                } else {
                    format!("{} in {}", cfg.routing.variable, cfg.routing.file)
                };
                println!(
                    "Proxy mode: point {url} at the proxy, once: host `host.docker.internal` from containers                      (`localhost` from this machine), port {}, database {}.
                     On Linux, containers also need `extra_hosts: [\"host.docker.internal:host-gateway\"]`                      and the proxy must listen on 0.0.0.0 (routing.listen).",
                    sidetrack_core::config::DEFAULT_PROXY_LISTEN
                        .rsplit(':')
                        .next()
                        .unwrap_or_default(),
                    found.active_db,
                );
                if cfg.routing.file.is_empty() {
                    println!(
                        "Git worktrees can't get their own database with the URL in the compose file.                          For that, move it to an untracked .env file and set routing.file and routing.variable."
                    );
                }
            }
            if opts.dry_run {
                println!("{}", cfg.to_toml()?);
                return Ok(true);
            }
            if !confirm(
                &format!(
                    "Write {} with these settings?",
                    sidetrack_core::config::CONFIG_FILE
                ),
                opts,
            )? {
                println!("Nothing changed.");
                return Ok(true);
            }
        }
    }

    let (ws, said) = setup.register(Registry::open(&registry::default_path()?)?)?;
    for line in said {
        println!("{line}");
    }
    if !no_hook
        && !hook::is_installed(&ws.git)
        && confirm(
            "Install the git hook, so switching branches offers to switch the database?",
            opts,
        )?
    {
        println!("Hook {}.", hook::install(&ws.git)?);
    }
    println!("Ready. Try `sidetrack status`.");
    Ok(true)
}

// ---------------------------------------------------------------------- status

fn status(dir: &Path, opts: &Opts) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    let branch = ws.git.current_branch()?;
    let active = ws.active_db()?;
    let world = match ws.world() {
        Ok(w) => w,
        Err(e) => {
            if opts.json {
                println!(
                    "{}",
                    json!({ "git_branch": branch, "active_db": active, "error": format!("{e:#}") })
                );
            } else {
                println!("Git branch:  {}", branch.as_deref().unwrap_or("(detached)"));
                println!("Database:    {}", active.as_deref().unwrap_or("?"));
                println!("! {e:#}");
            }
            return Ok(false);
        }
    };
    let planner = ws.planner(&world);
    let linked = active
        .as_deref()
        .and_then(|a| planner.branch_for_db(a))
        .map(|b| b.git_branch.clone());
    let expected = branch
        .as_deref()
        .and_then(|b| planner.branch_for_git(b))
        .map(|b| b.db_name.clone());
    let plan = match &branch {
        Some(b) => {
            let mut p = planner.switch(b)?;
            ws.annotate_schema(&mut p);
            Some(p)
        }
        None => None,
    };
    let migrations_status = match &branch {
        Some(b) => planner.migration_status(b)?,
        None => None,
    };
    let in_sync = plan.as_ref().is_some_and(|p| p.is_noop());

    if opts.json {
        println!(
            "{}",
            json!({
                "git_branch": branch, "active_db": active, "linked_branch": linked,
                "expected_db": expected, "in_sync": in_sync, "warnings": planner.reconcile(), "plan": plan,
                "migrations": migrations_status, "reader_error": world.reader_error,
            })
        );
        return Ok(true);
    }
    println!("Git branch:  {}", branch.as_deref().unwrap_or("(detached)"));
    if ws.is_linked_worktree() {
        println!(
            "Worktree:    {} (shares databases with {})",
            ws.root.display(),
            ws.main_root.display()
        );
    }
    if let Some(up) = world.proxy_running {
        println!(
            "Proxy:       {} on {}{}",
            ws.cfg.routing.database.as_deref().unwrap_or("?"),
            ws.proxy_listen(),
            if up {
                ""
            } else {
                "  ✗ not running (`sidetrack proxy`)"
            }
        );
    }
    println!(
        "Database:    {}{}",
        active.as_deref().unwrap_or("?"),
        linked
            .map(|l| format!("  (DB branch of {l})"))
            .unwrap_or_default()
    );
    if let Some(b) = &branch {
        println!(
            "Expected:    {}",
            expected
                .unwrap_or_else(|| format!("{} (not created yet)", naming::db_name_for_branch(b)))
        );
    }
    if let Some(m) = &migrations_status {
        println!(
            "Migrations:  {} applied in {} ({}) · {} pending · {} unknown to this branch",
            m.applied,
            m.db_name,
            m.source,
            m.pending.len(),
            m.unknown.len()
        );
    }
    for (db, wt) in &world.elsewhere {
        println!("Elsewhere:   {db} is in use in the worktree at {wt}");
    }
    if let Some(e) = &world.reader_error {
        println!(
            "  ! couldn't read the migration version table, so sidetrack's records were used: {e}"
        );
    }
    match &plan {
        Some(p) if p.is_noop() => {
            println!("State:       ✓ in sync");
            for w in &p.warnings {
                println!("  ! {w}");
            }
        }
        Some(p) => {
            println!("State:       ✗ out of sync — `sidetrack switch` would:\n");
            print!("{}", p.render(opts.verbose));
        }
        None => println!("State:       detached HEAD; nothing to compare"),
    }
    Ok(true)
}

// ------------------------------------------------------------------------ list

fn list(dir: &Path, opts: &Opts) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    let world = ws.world()?;
    if opts.json {
        println!(
            "{}",
            json!({ "active_db": world.active_db, "branches": world.branches, "snapshots": world.snapshots, "server_dbs": world.server_dbs })
        );
        return Ok(true);
    }
    if world.branches.is_empty() {
        println!("No DB branches yet. Run `sidetrack init`.");
        return Ok(true);
    }
    let width = world
        .branches
        .iter()
        .map(|b| b.git_branch.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let dbw = world
        .branches
        .iter()
        .map(|b| b.db_name.len())
        .max()
        .unwrap_or(8)
        .max(8);
    println!(
        "  {:width$}  {:dbw$}  {:>9}  {:>10}  LAST USED",
        "BRANCH", "DATABASE", "SIZE", "MIGRATIONS"
    );
    for b in &world.branches {
        let mark = if world.active_db.as_deref() == Some(b.db_name.as_str()) {
            "*"
        } else {
            " "
        };
        let size = world
            .server_dbs
            .get(&b.db_name)
            .map(|d| format_bytes(d.size_bytes))
            .unwrap_or_else(|| "missing".into());
        println!(
            "{mark} {:width$}  {:dbw$}  {size:>9}  {:>10}  {}",
            b.git_branch,
            b.db_name,
            b.migration_files.len(),
            ago(b.last_used_at)
        );
        for s in world.snapshots.iter().filter(|s| s.branch_id == b.id) {
            let size = world
                .server_dbs
                .get(&s.db_name)
                .map(|d| format_bytes(d.size_bytes))
                .unwrap_or_else(|| "missing".into());
            let label = s
                .label
                .as_deref()
                .map(|l| format!(" \"{l}\""))
                .unwrap_or_default();
            println!(
                "    ↳ snapshot {}{label}  {size}  {}",
                s.db_name,
                ago(s.created_at)
            );
        }
    }
    Ok(true)
}

fn snapshot_list(dir: &Path, opts: &Opts) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    let project = ws.project()?;
    let snaps = ws.reg.snapshots(project.id)?;
    if opts.json {
        println!("{}", json!({ "snapshots": snaps }));
    } else if snaps.is_empty() {
        println!("No snapshots. Create one with `sidetrack snapshot [label]`.");
    } else {
        for s in snaps {
            let label = s
                .label
                .as_deref()
                .map(|l| format!(" \"{l}\""))
                .unwrap_or_default();
            println!(
                "{}{label}  {} migrations  {}",
                s.db_name,
                s.migration_files.len(),
                ago(s.created_at)
            );
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------- doctor

fn doctor(dir: &Path, opts: &Opts) -> Result<bool> {
    let mut checks: Vec<(String, bool, String)> = vec![];
    let mut check =
        |name: &str, ok: bool, detail: String| checks.push((name.to_string(), ok, detail));

    let git = match Git::discover(dir) {
        Ok(g) => {
            check("git repository", true, g.root().display().to_string());
            g
        }
        Err(e) => {
            check("git repository", false, format!("{e:#}"));
            return finish_doctor(checks, opts);
        }
    };
    let cfg = match Config::load(git.root()) {
        Ok(c) => {
            check("sidetrack.toml", true, "valid".into());
            c
        }
        Err(e) => {
            check("sidetrack.toml", false, format!("{e:#}"));
            return finish_doctor(checks, opts);
        }
    };
    let reg_path = registry::default_path()?;
    let ws = Workspace::from_parts(git, cfg, Registry::open(&reg_path)?);
    check(
        "registry",
        ws.project().is_ok(),
        format!(
            "{} ({})",
            reg_path.display(),
            if ws.project().is_ok() {
                "repo registered"
            } else {
                "run `sidetrack init`"
            }
        ),
    );
    check(
        "git hook",
        hook::is_installed(&ws.git),
        if hook::is_installed(&ws.git) {
            "installed".into()
        } else {
            "not installed (`sidetrack hook install`)".into()
        },
    );

    if ws.cfg.routing.is_proxy() {
        let target = ws.active_db().ok().flatten().unwrap_or_default();
        let up = ws.proxy_running();
        check(
            "proxy",
            up,
            if up {
                format!("{} → {target}", ws.proxy_listen())
            } else {
                format!(
                    "nothing listening on {}; start it with `sidetrack proxy`",
                    ws.proxy_listen()
                )
            },
        );
        match ws.proxy_upstream() {
            Ok(u) => check("proxy upstream", true, u),
            Err(e) => check("proxy upstream", false, format!("{e:#}")),
        }
    } else {
        match ws.active_db() {
            Ok(Some(db)) => check(
                "env file",
                true,
                format!(
                    "{} in {} → {db}",
                    ws.cfg.routing.variable, ws.cfg.routing.file
                ),
            ),
            Ok(None) => check(
                "env file",
                false,
                format!(
                    "no database name in {} in {}",
                    ws.cfg.routing.variable, ws.cfg.routing.file
                ),
            ),
            Err(e) => check("env file", false, format!("{e:#}")),
        }
        let tracked = ws.git.is_tracked(&ws.cfg.routing.file).unwrap_or(false);
        check(
            "env file untracked",
            !tracked,
            if tracked {
                "tracked by git; sidetrack won't rewrite it".into()
            } else {
                "ok".into()
            },
        );
    }
    let missing: Vec<&String> = ws
        .cfg
        .migrations
        .dirs
        .iter()
        .filter(|d| !ws.root.join(d).is_dir())
        .collect();
    check(
        "migration dirs",
        missing.is_empty(),
        if missing.is_empty() {
            ws.cfg.migrations.dirs.join(", ")
        } else {
            format!("missing: {missing:?}")
        },
    );

    if !docker_available() {
        check("docker", false, "not reachable".into());
        return finish_doctor(checks, opts);
    }
    check("docker", true, "reachable".into());
    let running = ws.compose.running_services().unwrap_or_default();
    let db_up = ws
        .db_compose
        .running_services()
        .unwrap_or_default()
        .contains(&ws.cfg.database.service);
    check(
        "database service",
        db_up,
        if db_up {
            format!("{} running", ws.cfg.database.service)
        } else {
            format!("{} not running", ws.cfg.database.service)
        },
    );
    if db_up {
        match ws.pg.server_version_num() {
            Ok(v) => {
                let how = match v {
                    v if v >= 180000 => {
                        "Postgres 18: clone copies (copy-on-write on btrfs/XFS/APFS)"
                    }
                    v if v >= 150000 => "file copies (STRATEGY FILE_COPY)",
                    _ => "template copies (upgrade to 15+ for faster copies)",
                };
                check(
                    "postgres",
                    true,
                    format!("version {}.{} — {how}", v / 10000, v % 10000),
                );
            }
            Err(e) => check("postgres", false, format!("{e:#}")),
        }
        if let Some(disk) = ws.disk_space() {
            check(
                "disk space",
                disk.free_bytes >= 1 << 30,
                format!(
                    "{} free of {} (each DB branch is a full copy)",
                    format_bytes(disk.free_bytes),
                    format_bytes(disk.total_bytes)
                ),
            );
        }
    }
    let app_down: Vec<&String> = ws
        .cfg
        .routing
        .restart
        .iter()
        .filter(|s| !running.contains(s))
        .collect();
    check(
        "app services",
        app_down.is_empty(),
        if app_down.is_empty() {
            ws.cfg.routing.restart.join(", ")
        } else {
            format!("not running: {app_down:?}")
        },
    );
    finish_doctor(checks, opts)
}

fn finish_doctor(checks: Vec<(String, bool, String)>, opts: &Opts) -> Result<bool> {
    let ok = checks.iter().all(|(_, ok, _)| *ok);
    if opts.json {
        let list: Vec<_> = checks
            .iter()
            .map(|(n, ok, d)| json!({ "check": n, "ok": ok, "detail": d }))
            .collect();
        println!("{}", json!({ "ok": ok, "checks": list }));
    } else {
        for (name, ok, detail) in &checks {
            println!("{} {name:<20} {detail}", if *ok { "✓" } else { "✗" });
        }
    }
    Ok(ok)
}

// ----------------------------------------------------------------------- bench

/// Copy speed and projected copy times for common database sizes.
fn bench_summary(plan: &Plan, report: &Report) -> Option<(String, serde_json::Value)> {
    let (index, source, size) = plan.steps.iter().enumerate().find_map(|(i, s)| match s {
        Step::CloneDatabase {
            source, size_bytes, ..
        } => Some((i, source.clone(), *size_bytes)),
        _ => None,
    })?;
    let timing = report.timings.iter().find(|t| t.step == index)?;
    let seconds = timing.seconds.max(0.001);
    let rate = size as f64 / seconds; // bytes per second
    let gib = 1u64 << 30;
    let mut sizes = vec![size, gib, 2 * gib, 5 * gib, 10 * gib];
    sizes.sort_unstable();
    sizes.dedup();
    let mut text = format!(
        "\nCopied {source} ({}) in {} — {}/s{}.\nAt this speed a copy would take about:\n",
        format_bytes(size),
        duration(seconds),
        format_bytes(rate as u64),
        timing
            .detail
            .as_deref()
            .map(|d| format!(", {d}"))
            .unwrap_or_default()
    );
    let mut projections = vec![];
    if size < 50 << 20 {
        // Fixed costs (a checkpoint, catalog work) dominate tiny copies, so a
        // straight-line estimate for big databases would be far too slow.
        text = format!(
            "\nCopied {source} ({}) in {}{}.\n\
             It's too small to estimate bigger copies from: fixed costs dominate. \
             Run `sidetrack bench --from <a realistic database>` for a real number.\n",
            format_bytes(size),
            duration(seconds),
            timing
                .detail
                .as_deref()
                .map(|d| format!(", {d}"))
                .unwrap_or_default()
        );
    } else {
        for s in sizes {
            let est = s as f64 / rate;
            text.push_str(&format!("  {:>9}  {}\n", format_bytes(s), duration(est)));
            projections.push(json!({ "size_bytes": s, "seconds": est }));
        }
        text.push_str(
            "The first copy after heavy writes can be slower: Postgres flushes recent changes \
             to disk before copying.\n",
        );
    }
    let value = json!({
        "source": source,
        "size_bytes": size,
        "seconds": seconds,
        "bytes_per_second": rate,
        "method": timing.detail,
        "projections": projections,
    });
    Some((text, value))
}

fn duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let s = seconds.round() as u64;
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// `sidetrack proxy`: route the app's connections to the active DB branch.
fn proxy(dir: &Path, listen: Option<String>) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    anyhow::ensure!(
        ws.cfg.routing.is_proxy(),
        "routing.mode in sidetrack.toml is {:?}; set it to \"proxy\" (and routing.database) to use the proxy",
        ws.cfg.routing.mode
    );
    let app_db = ws.cfg.routing.database.clone().unwrap_or_default();
    let listen = listen.unwrap_or_else(|| ws.proxy_listen());
    let upstream = ws.proxy_upstream()?;
    let listener = std::net::TcpListener::bind(&listen)
        .with_context(|| format!("can't listen on {listen} (is another proxy running?)"))?;
    let target_file = ws.proxy_target_path();
    // One certificate per machine, next to the registry, so clients that trust
    // it work with every project and worktree. Without it the proxy still runs,
    // answering SSL requests with "no" as before.
    let tls_file = registry::default_path()?.with_file_name("proxy-tls.pem");
    let tls = match sidetrack_core::proxy::tls_config(&tls_file) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!(
                "sidetrack proxy: TLS is off, so clients with sslmode=require can't connect \
                 ({}: {e}). Fix or delete that file, then restart the proxy.",
                tls_file.display()
            );
            None
        }
    };
    eprintln!(
        "sidetrack proxy: {listen} → {upstream}; {app_db} → {} (Ctrl-C to stop)",
        sidetrack_core::proxy::read_target(&target_file, &app_db)
    );
    let p = sidetrack_core::proxy::Proxy {
        app_db,
        upstream,
        target_file,
        hold_file: ws.proxy_hold_path(),
        poll: std::time::Duration::from_millis(250),
        tls,
    };
    p.serve(
        listener,
        std::sync::Arc::new(|m: &str| eprintln!("sidetrack proxy: {m}")),
    )?;
    Ok(true)
}

/// `sidetrack proxy --daemon`: start the proxy in the background and wait until it listens.
fn proxy_daemon(dir: &Path, listen: Option<String>) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    anyhow::ensure!(
        ws.cfg.routing.is_proxy(),
        "routing.mode in sidetrack.toml isn't \"proxy\""
    );
    if listen.is_none() && ws.proxy_running() {
        println!("The proxy is already running on {}.", ws.proxy_listen());
        return Ok(true);
    }
    println!(
        "{}",
        ws.start_proxy(&std::env::current_exe()?, listen.as_deref())?
    );
    Ok(true)
}

/// Proxy mode with autostart on: start the proxy before planning, so plans can
/// hold connections instead of stopping the app. Never fails the command.
pub(crate) fn autostart_proxy(ws: &Workspace) {
    if ws.proxy_needs_start() {
        let started = std::env::current_exe()
            .map_err(anyhow::Error::from)
            .and_then(|exe| ws.start_proxy(&exe, None));
        match started {
            Ok(msg) => eprintln!("sidetrack: {msg}"),
            Err(e) => eprintln!("sidetrack: couldn't start the proxy: {e:#}"),
        }
    }
}

/// `sidetrack proxy --stop`.
fn proxy_stop(dir: &Path) -> Result<bool> {
    let ws = Workspace::open(dir)?;
    let (pid_file, _) = ws.proxy_files()?;
    let Ok(pid) = std::fs::read_to_string(&pid_file) else {
        if ws.proxy_running() {
            bail!(
                "something is listening on {}, but it wasn't started with `sidetrack proxy --daemon`; stop it where it runs",
                ws.proxy_listen()
            );
        }
        println!("The proxy isn't running.");
        return Ok(true);
    };
    let pid = pid.trim();
    let killed = std::process::Command::new("kill")
        .arg(pid)
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    std::fs::remove_file(&pid_file)?;
    if killed {
        println!("Stopped the proxy (pid {pid}).");
    } else {
        println!("The proxy (pid {pid}) had already stopped.");
    }
    Ok(true)
}
