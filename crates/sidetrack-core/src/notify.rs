//! Telling the desktop app that a checkout needs a switch (spec P2, "approvals").
//!
//! The app listens on a localhost TCP port and writes it to `app.port` next to
//! the registry. The git hook, when it has no terminal to ask in, sends the
//! project's registry key there as one line. The app answers `ok` if it shows
//! the switch, or `no` if that root isn't a registered project. The app builds
//! the plan itself and applies only what the user approves there, so nothing
//! but the root crosses the socket, and the hook never waits for a decision.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::registry;

/// Each wait on the socket is capped, so a stale port file (the app crashed,
/// another program took the port) costs the hook well under a second.
const TIMEOUT: Duration = Duration::from_millis(300);

/// `app.port`, next to the registry.
pub fn port_file() -> Result<PathBuf> {
    Ok(registry::default_path()?.with_file_name("app.port"))
}

/// Ask the running app to show the switch for `root`. True only if the app
/// answered that it will; false if it isn't running or doesn't know `root`.
pub fn send(root: &str) -> bool {
    port_file().is_ok_and(|f| send_to(&f, root))
}

pub fn send_to(port_file: &Path, root: &str) -> bool {
    let attempt = || -> Result<bool> {
        let port: u16 = std::fs::read_to_string(port_file)?.trim().parse()?;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
        s.set_read_timeout(Some(TIMEOUT))?;
        s.set_write_timeout(Some(TIMEOUT))?;
        writeln!(s, "{root}")?;
        let mut answer = String::new();
        BufReader::new(s.take(16)).read_line(&mut answer)?;
        Ok(answer.trim() == "ok")
    };
    !root.contains('\n') && attempt().unwrap_or(false)
}

/// Listen for checkouts and write the port to `port_file`. `on_checkout` gets
/// each root and returns whether it will show it. Runs on a background thread
/// for the life of the process.
pub fn listen(port_file: &Path, on_checkout: impl Fn(&str) -> bool + Send + 'static) -> Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    if let Some(dir) = port_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(port_file, format!("{port}\n"))
        .with_context(|| format!("writing {}", port_file.display()))?;
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let _ = answer(conn, &on_checkout);
        }
    });
    Ok(())
}

fn answer(mut conn: TcpStream, on_checkout: &impl Fn(&str) -> bool) -> Result<()> {
    conn.set_read_timeout(Some(TIMEOUT))?;
    conn.set_write_timeout(Some(TIMEOUT))?;
    let mut line = String::new();
    BufReader::new((&conn).take(4096)).read_line(&mut line)?;
    let root = line.trim_end_matches(['\r', '\n']);
    let ok = !root.is_empty() && on_checkout(root);
    conn.write_all(if ok { b"ok\n" } else { b"no\n" })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn the_app_answers_for_known_roots_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.port");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        listen(&file, move |root| {
            tx.lock().unwrap().send(root.to_string()).unwrap();
            root == "/repos/shop"
        })
        .unwrap();

        assert!(send_to(&file, "/repos/shop"));
        assert_eq!(rx.recv().unwrap(), "/repos/shop");
        assert!(!send_to(&file, "/repos/other"));
        assert_eq!(rx.recv().unwrap(), "/repos/other");
    }

    #[test]
    fn no_app_means_false_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.port");
        assert!(!send_to(&file, "/repos/shop"), "no port file");

        // A port file left behind by an app that has exited.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(&file, format!("{port}\n")).unwrap();
        let start = std::time::Instant::now();
        assert!(!send_to(&file, "/repos/shop"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
