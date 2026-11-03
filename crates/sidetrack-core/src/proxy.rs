//! Local Postgres proxy for `routing.mode = "proxy"` (spec P2).
//!
//! The app always connects to its usual database name (`routing.database`) through
//! this proxy. The proxy reads the client's startup message, swaps that name for the
//! active DB branch, then relays bytes both ways untouched, authentication included,
//! so it never sees a password.
//!
//! The active DB branch lives in a small file (`Workspace::proxy_target_path`) that
//! `switch` rewrites. A watcher notices the change and closes the app's connections
//! to the old database, so connection pools reconnect to the new one. Connections
//! to other database names pass through untouched and are never closed.
//!
//! TLS isn't supported: SSL and GSS encryption requests are answered "no", which
//! clients using the libpq default (`sslmode=prefer`) accept.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const SSL_REQUEST: u32 = 80_877_103;
const GSSENC_REQUEST: u32 = 80_877_104;
const CANCEL_REQUEST: u32 = 80_877_102;
/// Postgres itself rejects startup packets over 10000 bytes.
const MAX_STARTUP_LEN: usize = 10_000;

/// The first message a client sends.
#[derive(Debug, Clone, PartialEq)]
pub enum Startup {
    SslRequest,
    GssEncRequest,
    /// Cancels a query on another connection; forwarded as is.
    Cancel(Vec<u8>),
    Params {
        protocol: u32,
        params: Vec<(Vec<u8>, Vec<u8>)>,
    },
}

impl Startup {
    pub fn param(&self, key: &str) -> Option<&[u8]> {
        match self {
            Startup::Params { params, .. } => params
                .iter()
                .find(|(k, _)| k == key.as_bytes())
                .map(|(_, v)| v.as_slice()),
            _ => None,
        }
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read one length-prefixed startup-phase message (length included).
pub fn read_startup(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if !(8..=MAX_STARTUP_LEN).contains(&n) {
        return Err(invalid("not a Postgres startup message"));
    }
    let mut msg = vec![0u8; n];
    msg[..4].copy_from_slice(&len);
    r.read_exact(&mut msg[4..])?;
    Ok(msg)
}

pub fn parse_startup(msg: &[u8]) -> io::Result<Startup> {
    if msg.len() < 8 {
        return Err(invalid("startup message too short"));
    }
    let code = u32::from_be_bytes([msg[4], msg[5], msg[6], msg[7]]);
    match code {
        SSL_REQUEST => return Ok(Startup::SslRequest),
        GSSENC_REQUEST => return Ok(Startup::GssEncRequest),
        CANCEL_REQUEST => return Ok(Startup::Cancel(msg.to_vec())),
        _ => {}
    }
    let mut params = Vec::new();
    let mut rest = &msg[8..];
    loop {
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated startup parameter"))?;
        if end == 0 {
            break;
        }
        let key = rest[..end].to_vec();
        rest = &rest[end + 1..];
        let vend = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated startup parameter"))?;
        params.push((key, rest[..vend].to_vec()));
        rest = &rest[vend + 1..];
    }
    Ok(Startup::Params {
        protocol: code,
        params,
    })
}

pub fn encode_startup(protocol: u32, params: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut body = protocol.to_be_bytes().to_vec();
    for (k, v) in params {
        body.extend_from_slice(k);
        body.push(0);
        body.extend_from_slice(v);
        body.push(0);
    }
    body.push(0);
    let mut msg = ((body.len() + 4) as u32).to_be_bytes().to_vec();
    msg.extend(body);
    msg
}

/// Swap `database = app_db` for `target`. Returns whether it was swapped.
pub fn reroute(startup: &mut Startup, app_db: &str, target: &str) -> bool {
    let Startup::Params { params, .. } = startup else {
        return false;
    };
    for (k, v) in params.iter_mut() {
        if k == b"database" && v == app_db.as_bytes() {
            *v = target.as_bytes().to_vec();
            return true;
        }
    }
    false
}

/// A FATAL `ErrorResponse`, so clients show a readable reason.
pub fn error_response(message: &str) -> Vec<u8> {
    let mut body = Vec::new();
    for (field, value) in [
        (b'S', "FATAL"),
        (b'V', "FATAL"),
        (b'C', "08006"),
        (b'M', message),
    ] {
        body.push(field);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
    }
    body.push(0);
    let mut msg = vec![b'E'];
    msg.extend(((body.len() + 4) as u32).to_be_bytes());
    msg.extend(body);
    msg
}

/// Reads the current target: the active DB branch, or `app_db` if none is set.
pub fn read_target(path: &std::path::Path, app_db: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| app_db.to_string())
}

/// Write the target atomically (the proxy may read it at any moment).
pub fn write_target(path: &std::path::Path, db: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{db}\n"))?;
    std::fs::rename(tmp, path)
}

/// File that asks the proxy to hold new connections to one database while it's
/// being copied (next to the target file).
pub fn hold_path(target_file: &std::path::Path) -> PathBuf {
    target_file.with_file_name("sidetrack-proxy-hold")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Hold new connections to `db` for at most `ttl` (a safety net if sidetrack dies
/// before releasing them).
pub fn write_hold(path: &std::path::Path, db: &str, ttl: Duration) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{db} {}\n", now_secs() + ttl.as_secs()))?;
    std::fs::rename(tmp, path)
}

/// The database whose new connections are held, unless the hold expired.
pub fn read_hold(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let (db, until) = text.trim().split_once(' ')?;
    (until.parse::<u64>().ok()? > now_secs()).then(|| db.to_string())
}

pub fn release_hold(path: &std::path::Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub struct Proxy {
    /// The database name the app connects to.
    pub app_db: String,
    /// Postgres address, e.g. `127.0.0.1:5432`.
    pub upstream: String,
    /// File holding the active DB branch.
    pub target_file: PathBuf,
    /// Hold file, shared by every worktree's proxy (`hold_path`'s location in
    /// a single checkout).
    pub hold_file: PathBuf,
    /// How often the watcher re-reads `target_file`.
    pub poll: Duration,
}

struct Routed {
    db: String,
    client: TcpStream,
    server: TcpStream,
}

type Log = Arc<dyn Fn(&str) + Send + Sync>;

impl Proxy {
    /// Accept connections forever.
    pub fn serve(self, listener: TcpListener, log: Log) -> io::Result<()> {
        let proxy = Arc::new(self);
        let routed: Arc<Mutex<HashMap<u64, Routed>>> = Arc::default();
        {
            let (proxy, routed, log) = (proxy.clone(), routed.clone(), log.clone());
            thread::spawn(move || proxy.watch(&routed, &log));
        }
        let ids = AtomicU64::new(0);
        for client in listener.incoming() {
            let client = match client {
                Ok(c) => c,
                Err(e) => {
                    log(&format!("accept failed: {e}"));
                    continue;
                }
            };
            let id = ids.fetch_add(1, Ordering::Relaxed);
            let (proxy, routed, log) = (proxy.clone(), routed.clone(), log.clone());
            thread::spawn(move || {
                if let Err(e) = proxy.handle(id, client, &routed, &log) {
                    log(&format!("connection {id}: {e}"));
                }
                routed.lock().unwrap().remove(&id);
            });
        }
        Ok(())
    }

    /// Close the app's connections to a database that's no longer the target.
    fn watch(&self, routed: &Mutex<HashMap<u64, Routed>>, log: &Log) {
        let mut last = read_target(&self.target_file, &self.app_db);
        loop {
            thread::sleep(self.poll);
            let now = read_target(&self.target_file, &self.app_db);
            if now == last {
                continue;
            }
            let mut closed = 0;
            for r in routed.lock().unwrap().values() {
                if r.db != now {
                    let _ = r.client.shutdown(Shutdown::Both);
                    let _ = r.server.shutdown(Shutdown::Both);
                    closed += 1;
                }
            }
            log(&format!(
                "{} → {now} (was {last}); closed {closed} connection(s)",
                self.app_db
            ));
            last = now;
        }
    }

    fn handle(
        &self,
        id: u64,
        mut client: TcpStream,
        routed: &Mutex<HashMap<u64, Routed>>,
        log: &Log,
    ) -> io::Result<()> {
        client.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut startup = loop {
            let msg = read_startup(&mut client)?;
            match parse_startup(&msg)? {
                Startup::SslRequest | Startup::GssEncRequest => client.write_all(b"N")?,
                Startup::Cancel(raw) => {
                    TcpStream::connect(&self.upstream)?.write_all(&raw)?;
                    return Ok(());
                }
                s => break s,
            }
        };
        let Startup::Params { protocol, .. } = startup else {
            unreachable!()
        };
        if protocol >> 16 != 3 {
            client.write_all(&error_response("sidetrack proxy: unsupported protocol"))?;
            return Ok(());
        }
        // Wait while the database this connection would reach is being copied.
        let asked = startup.param("database").map(|d| d.to_vec());
        let hold_file = &self.hold_file;
        let mut held = false;
        let target = loop {
            let target = read_target(&self.target_file, &self.app_db);
            let reaches = match &asked {
                Some(d) if d == self.app_db.as_bytes() => target.clone().into_bytes(),
                Some(d) => d.clone(),
                None => vec![],
            };
            match read_hold(hold_file) {
                Some(h) if h.as_bytes() == reaches.as_slice() => {
                    if !held {
                        log(&format!("connection {id}: holding while {h} is copied"));
                        held = true;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                _ => break target,
            }
        };
        let rerouted = reroute(&mut startup, &self.app_db, &target);
        let mut server = match TcpStream::connect(&self.upstream) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!(
                    "sidetrack proxy can't reach Postgres at {}: {e}",
                    self.upstream
                );
                client.write_all(&error_response(&msg))?;
                return Err(io::Error::other(msg));
            }
        };
        let Startup::Params { params, .. } = &startup else {
            unreachable!()
        };
        server.write_all(&encode_startup(protocol, params))?;
        client.set_read_timeout(None)?;
        let _ = client.set_nodelay(true);
        let _ = server.set_nodelay(true);
        if rerouted {
            log(&format!("connection {id}: {} → {target}", self.app_db));
            routed.lock().unwrap().insert(
                id,
                Routed {
                    db: target,
                    client: client.try_clone()?,
                    server: server.try_clone()?,
                },
            );
        }
        relay(client, server)
    }
}

/// Copy bytes both ways until either side closes.
fn relay(mut client: TcpStream, mut server: TcpStream) -> io::Result<()> {
    let (mut c2, mut s2) = (client.try_clone()?, server.try_clone()?);
    let up = thread::spawn(move || {
        let _ = io::copy(&mut c2, &mut s2);
        let _ = s2.shutdown(Shutdown::Both);
    });
    let _ = io::copy(&mut server, &mut client);
    let _ = client.shutdown(Shutdown::Both);
    let _ = server.shutdown(Shutdown::Both);
    let _ = up.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        pairs
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn startup_round_trip_and_reroute() {
        let raw = encode_startup(
            196_608,
            &params(&[("user", "app"), ("database", "app_dev")]),
        );
        let mut s = parse_startup(&read_startup(&mut raw.as_slice()).unwrap()).unwrap();
        assert_eq!(s.param("database"), Some(&b"app_dev"[..]));
        assert!(reroute(&mut s, "app_dev", "feature_a_db"));
        assert_eq!(s.param("database"), Some(&b"feature_a_db"[..]));
        assert_eq!(s.param("user"), Some(&b"app"[..]));

        let mut other =
            parse_startup(&encode_startup(196_608, &params(&[("database", "x")]))).unwrap();
        assert!(!reroute(&mut other, "app_dev", "feature_a_db"));

        let ssl = [0, 0, 0, 8, 4, 210, 22, 47];
        assert_eq!(parse_startup(&ssl).unwrap(), Startup::SslRequest);
        assert!(read_startup(&mut &[0u8, 0, 0, 2][..]).is_err());
        assert!(parse_startup(&[0, 0, 0, 9, 0, 3, 0, 0, b'x']).is_err());
    }

    #[test]
    fn error_response_is_well_formed() {
        let e = error_response("nope");
        assert_eq!(e[0], b'E');
        let len = u32::from_be_bytes([e[1], e[2], e[3], e[4]]) as usize;
        assert_eq!(len + 1, e.len());
        assert!(e.ends_with(b"Mnope\0\0"));
    }

    /// A fake Postgres that reports the database it was asked for, then echoes.
    fn fake_upstream() -> (String, std::sync::mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let tx = tx.clone();
                thread::spawn(move || {
                    let st = parse_startup(&read_startup(&mut s).unwrap()).unwrap();
                    let db = String::from_utf8(st.param("database").unwrap().to_vec()).unwrap();
                    tx.send(db).unwrap();
                    let mut buf = [0u8; 64];
                    while let Ok(n) = s.read(&mut buf) {
                        if n == 0 || s.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                });
            }
        });
        (addr, rx)
    }

    fn connect(addr: &str, db: &str) -> TcpStream {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).unwrap(); // SSLRequest
        let mut n = [0u8; 1];
        c.read_exact(&mut n).unwrap();
        assert_eq!(&n, b"N");
        c.write_all(&encode_startup(
            196_608,
            &params(&[("user", "app"), ("database", db)]),
        ))
        .unwrap();
        c
    }

    #[test]
    fn routes_to_target_and_moves_connections_on_switch() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let (upstream, seen) = fake_upstream();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let proxy = Proxy {
            app_db: "app_dev".into(),
            upstream,
            target_file: target.clone(),
            hold_file: hold_path(&target),
            poll: Duration::from_millis(20),
        };
        thread::spawn(move || proxy.serve(listener, Arc::new(|_: &str| {})));
        let t = Duration::from_secs(5);

        // No target yet: app_dev goes to app_dev.
        let mut a = connect(&addr, "app_dev");
        assert_eq!(seen.recv_timeout(t).unwrap(), "app_dev");
        a.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        a.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");

        // Another database name passes through untouched.
        let mut other = connect(&addr, "postgres");
        assert_eq!(seen.recv_timeout(t).unwrap(), "postgres");

        // Switch: the app's old connection is closed, new ones go to the branch.
        write_target(&target, "feature_a_db").unwrap();
        let mut rest = Vec::new();
        a.read_to_end(&mut rest).unwrap(); // returns once the proxy closes it
        let _b = connect(&addr, "app_dev");
        assert_eq!(seen.recv_timeout(t).unwrap(), "feature_a_db");

        // A hold on feature_a_db makes new app connections wait until it's released.
        let hold = hold_path(&target);
        write_hold(&hold, "feature_a_db", Duration::from_secs(60)).unwrap();
        let _c = connect(&addr, "app_dev");
        assert!(
            seen.recv_timeout(Duration::from_millis(300)).is_err(),
            "held"
        );
        release_hold(&hold).unwrap();
        assert_eq!(seen.recv_timeout(t).unwrap(), "feature_a_db");
        write_hold(&hold, "feature_a_db", Duration::ZERO).unwrap();
        assert_eq!(read_hold(&hold), None, "an expired hold is ignored");

        other.write_all(b"pong").unwrap();
        other.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"pong", "connections to other databases stay open");
    }
}
