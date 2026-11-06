//! The game's side of the live socket.
//!
//! - **Endpoint.** A named pipe `\\.\pipe\galaxy-live-<pid>-<n>-<random>` on Windows, a
//!   Unix socket `<live folder>/<pid>-<n>.sock` elsewhere (in a private temporary folder
//!   when that path is too long for a socket). Named pipes refuse remote clients and their
//!   default access lets only the user (and administrators) in; the Unix socket is mode
//!   0600 in a folder only the user can open. Every request also carries the random token
//!   from the discovery file, which only the user can read.
//! - **Threads.** One thread accepts connections, without blocking, so it notices the
//!   server stop. Each connection gets two threads of its own: one reads request lines,
//!   hands each to the game through a channel and wakes the game; the other writes each
//!   reply when the game has answered. Reading never waits for an answer, so a client
//!   that hangs up is noticed at once and the game hears [`Message::Closed`]. The game
//!   answers between frames ([`Server::poll`]) and never waits on a client: a reply goes
//!   into a channel.
//! - **Limits.** At most [`MAX_CONNECTIONS`] at once; a request line is at most
//!   [`MAX_REQUEST`] bytes; a request with a wrong token closes the connection.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream};
use serde_json::Value;

use crate::protocol::{
    Discovery, Endpoint, EndpointKind, MAX_REQUEST, Op, PROTOCOL, Request, Response, RunningModule,
    WireError,
};

/// Connections open at once; one more is refused.
pub const MAX_CONNECTIONS: usize = 16;

/// Called from a connection thread when a request is waiting, so a game whose window loop
/// sleeps (minimized, or between events) wakes up to answer it.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A request waiting for the game. Answer it with [`Incoming::respond`], from any thread.
/// Dropping it unanswered tells the client the game went away.
#[derive(Debug)]
pub struct Incoming {
    pub op: Op,
    /// The connection it came on: what a [`Message::Closed`] names.
    pub conn: u64,
    id: u64,
    reply: Option<Sender<String>>,
}

impl Incoming {
    pub fn respond(mut self, result: Result<Value, WireError>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Response::line(self.id, result));
        }
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(gone(self.id));
        }
    }
}

/// What the game hears from its connections.
#[derive(Debug)]
pub enum Message {
    Request(Incoming),
    /// This connection closed (its client finished, or died): the game lets go of what
    /// it held.
    Closed(u64),
}

/// Where the server listens and what its discovery file says.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// The live folder: the discovery file goes here (and on Unix the socket).
    pub dir: PathBuf,
    /// The session folder.
    pub session_dir: PathBuf,
    /// The build that is playing.
    pub build: String,
    pub modules: Vec<RunningModule>,
}

/// The game's side of the socket. Dropping it stops accepting, ends every connection's
/// wait for the game and removes the discovery file.
pub struct Server {
    rx: Receiver<Message>,
    discovery: Discovery,
    file: PathBuf,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    #[cfg(unix)]
    _socket_dir: Option<tempfile::TempDir>,
}

/// Servers started by this process, so each gets its own names (tests start several).
static STARTED: AtomicUsize = AtomicUsize::new(0);

impl Server {
    /// Opens the endpoint, starts the accept thread and writes the discovery file.
    pub fn start(opts: ServerOptions, wake: Option<Wake>) -> io::Result<Server> {
        create_private_dir(&opts.dir)?;
        let pid = std::process::id();
        let n = STARTED.fetch_add(1, Ordering::SeqCst);
        let key = format!("{pid}-{n}");
        let token = random_hex(32)?;
        #[cfg(unix)]
        let mut socket_dir = None;
        #[cfg(windows)]
        let endpoint = Endpoint {
            kind: EndpointKind::NamedPipe,
            name: format!("galaxy-live-{key}-{}", random_hex(4)?),
        };
        #[cfg(unix)]
        let endpoint = {
            // Socket paths are short (about 100 bytes); fall back to a private temp folder.
            let mut path = opts.dir.join(format!("{key}.sock"));
            if path.as_os_str().len() > 100 {
                let dir = tempfile::Builder::new().prefix("galaxy-live-").tempdir()?;
                create_private_dir(dir.path())?;
                path = dir.path().join(format!("{key}.sock"));
                socket_dir = Some(dir);
            }
            let _ = std::fs::remove_file(&path);
            Endpoint {
                kind: EndpointKind::UnixSocket,
                name: path.to_string_lossy().into_owned(),
            }
        };
        let listener = {
            let options = ListenerOptions::new()
                .name(endpoint.to_name()?)
                .nonblocking(ListenerNonblockingMode::Accept);
            #[cfg(all(unix, not(target_os = "macos")))]
            let options = {
                use interprocess::os::unix::local_socket::ListenerOptionsExt;
                options.mode(0o600)
            };
            options.create_sync()?
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&endpoint.name, std::fs::Permissions::from_mode(0o600))?;
        }
        let session = opts
            .session_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let discovery = Discovery {
            protocol: PROTOCOL,
            endpoint,
            token: token.clone(),
            pid,
            session,
            session_dir: opts.session_dir,
            build: opts.build,
            modules: opts.modules,
            started: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        };
        let file = opts.dir.join(format!("{key}.json"));
        let text = serde_json::to_string_pretty(&discovery).expect("discovery serializes");
        write_private(&file, &text)?;

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let accept = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("galaxy-live".into())
                .spawn(move || accept_loop(listener, tx, token, stop, wake))?
        };
        Ok(Server {
            rx,
            discovery,
            file,
            stop,
            accept: Some(accept),
            #[cfg(unix)]
            _socket_dir: socket_dir,
        })
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// The next request waiting, if any. Never blocks.
    pub fn poll(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.file);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        if self.discovery.endpoint.kind == EndpointKind::UnixSocket {
            let _ = std::fs::remove_file(&self.discovery.endpoint.name);
        }
    }
}

fn accept_loop(
    listener: interprocess::local_socket::Listener,
    tx: Sender<Message>,
    token: String,
    stop: Arc<AtomicBool>,
    wake: Option<Wake>,
) {
    let open = Arc::new(AtomicUsize::new(0));
    let mut conns: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let stream = match listener.accept() {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let line = Response::line(
                0,
                Err(WireError::runtime(format!(
                    "the game already has {MAX_CONNECTIONS} live connections open; try again"
                ))),
            );
            let _ = (&stream).write_all(format!("{line}\n").as_bytes());
            continue;
        }
        open.fetch_add(1, Ordering::SeqCst);
        conns += 1;
        let conn = Conn {
            id: conns,
            tx: tx.clone(),
            token: token.clone(),
            wake: wake.clone(),
            stop: stop.clone(),
        };
        let counted = open.clone();
        let spawned = std::thread::Builder::new()
            .name("galaxy-live-conn".into())
            .spawn(move || {
                conn.serve(stream);
                counted.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            // The thread never ran, so it can't count itself out.
            open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

struct Conn {
    id: u64,
    tx: Sender<Message>,
    token: String,
    wake: Option<Wake>,
    stop: Arc<AtomicBool>,
}

impl Conn {
    /// Reads requests until the client hangs up, while a thread of its own writes the
    /// replies as the game gives them. Reading never waits for the game, so a client that
    /// goes away is noticed even while the game still works on its request; the game then
    /// hears [`Message::Closed`] and lets go of what the connection held.
    fn serve(self, stream: Stream) {
        let stream = Arc::new(stream);
        let send = stream.clone();
        let (reply_tx, reply_rx) = mpsc::channel::<String>();
        let writer = std::thread::Builder::new()
            .name("galaxy-live-reply".into())
            .spawn(move || {
                for line in reply_rx {
                    if write_line(&mut &*send, &line).is_err() {
                        return;
                    }
                }
            });
        if writer.is_err() {
            return;
        }
        let mut reader = BufReader::new(&*stream);
        loop {
            let mut line = Vec::new();
            match (&mut reader)
                .take(MAX_REQUEST + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if line.len() as u64 > MAX_REQUEST {
                let e = WireError::input(format!(
                    "a request is at most {MAX_REQUEST} bytes; this one is longer"
                ));
                let _ = reply_tx.send(Response::line(0, Err(e)));
                break;
            }
            if !self.request(&line, &reply_tx) {
                break;
            }
        }
        drop(reply_tx);
        if self.tx.send(Message::Closed(self.id)).is_ok()
            && let Some(wake) = &self.wake
        {
            wake();
        }
    }

    /// Hands one request line to the game, or replies to it here when it is no request
    /// the game can take. False when the connection should close.
    fn request(&self, line: &[u8], reply: &Sender<String>) -> bool {
        let request: Request = match serde_json::from_slice(line) {
            Ok(r) => r,
            Err(e) => {
                let e = WireError::input(format!("not a request: {e}"));
                let _ = reply.send(Response::line(0, Err(e)));
                return false;
            }
        };
        if !token_matches(&request.token, &self.token) {
            let e = WireError::runtime(
                "the live token is wrong: the game that wrote this discovery file is not \
                 the one listening",
            );
            let _ = reply.send(Response::line(request.id, Err(e)));
            return false;
        }
        let op = match Op::parse(&request.op, request.args) {
            Ok(op) => op,
            Err(e) => {
                let _ = reply.send(Response::line(request.id, Err(WireError::usage(e))));
                return true;
            }
        };
        let incoming = Incoming {
            op,
            conn: self.id,
            id: request.id,
            reply: Some(reply.clone()),
        };
        if self.stop.load(Ordering::SeqCst) {
            // Dropped unanswered: the client hears the game went away.
            drop(incoming);
            return false;
        }
        // The game answers every request it takes, or drops it (and so says it went away)
        // when it quits.
        if self.tx.send(Message::Request(incoming)).is_err() {
            return false;
        }
        if let Some(wake) = &self.wake {
            wake();
        }
        true
    }
}

fn gone(id: u64) -> String {
    Response::line(
        id,
        Err(WireError::runtime(
            "the game closed before it answered (its window was closed or it quit)",
        )),
    )
}

fn write_line(w: &mut impl Write, line: &str) -> io::Result<()> {
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

/// `bytes` random bytes from the OS, as hex.
fn random_hex(bytes: usize) -> io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Every byte compared, whatever the first difference.
fn token_matches(given: &str, want: &str) -> bool {
    given.len() == want.len()
        && given
            .bytes()
            .zip(want.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// A folder only the user can open (on Unix; Windows user folders already are).
fn create_private_dir(dir: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes a file only the user can read, whole or not at all.
fn write_private(path: &std::path::Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(text.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::token_matches;

    #[test]
    fn tokens_match_only_whole() {
        assert!(token_matches("abcd", "abcd"));
        assert!(!token_matches("abce", "abcd"));
        assert!(!token_matches("abc", "abcd"));
        assert!(!token_matches("", "abcd"));
    }
}
