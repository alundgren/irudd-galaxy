//! The game's side of the live socket.
//!
//! - **Endpoint.** A named pipe `\\.\pipe\galaxy-live-<pid>-<n>-<random>` on Windows, a
//!   Unix socket `<live folder>/<pid>-<n>.sock` elsewhere (in a private temporary folder
//!   when that path is too long for a socket). Named pipes refuse remote clients and their
//!   default access lets only the user (and administrators) in; the Unix socket is mode
//!   0600 in a folder only the user can open. Every request also carries the random token
//!   from the discovery file, which only the user can read.
//! - **Threads.** One thread accepts connections, without blocking, so it notices the
//!   server stop. Each connection gets a thread of its own that reads request lines,
//!   hands each to the game through a channel, wakes the game, and writes the reply when
//!   the game has answered. The game answers between frames ([`Server::poll`]) and never
//!   waits on a client: a reply goes into a one-place channel.
//! - **Limits.** At most [`MAX_CONNECTIONS`] at once; a request line is at most
//!   [`MAX_REQUEST`] bytes; a request with a wrong token closes the connection.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
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
    id: u64,
    reply: SyncSender<String>,
}

impl Incoming {
    pub fn respond(self, result: Result<Value, WireError>) {
        let _ = self.reply.try_send(Response::line(self.id, result));
    }
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
    rx: Receiver<Incoming>,
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
    pub fn poll(&self) -> Option<Incoming> {
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
    tx: Sender<Incoming>,
    token: String,
    stop: Arc<AtomicBool>,
    wake: Option<Wake>,
) {
    let open = Arc::new(AtomicUsize::new(0));
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
        let conn = Conn {
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
    tx: Sender<Incoming>,
    token: String,
    wake: Option<Wake>,
    stop: Arc<AtomicBool>,
}

impl Conn {
    fn serve(self, stream: Stream) {
        let mut reader = BufReader::new(&stream);
        loop {
            let mut line = Vec::new();
            match (&mut reader)
                .take(MAX_REQUEST + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line.len() as u64 > MAX_REQUEST {
                let e = WireError::input(format!(
                    "a request is at most {MAX_REQUEST} bytes; this one is longer"
                ));
                let _ = send(&stream, &Response::line(0, Err(e)));
                return;
            }
            let (reply, close) = self.answer(&line);
            if send(&stream, &reply).is_err() || close {
                return;
            }
        }
    }

    /// The reply line to one request line, and whether to close the connection after it.
    fn answer(&self, line: &[u8]) -> (String, bool) {
        let request: Request = match serde_json::from_slice(line) {
            Ok(r) => r,
            Err(e) => {
                let e = WireError::input(format!("not a request: {e}"));
                return (Response::line(0, Err(e)), true);
            }
        };
        if !token_matches(&request.token, &self.token) {
            let e = WireError::runtime(
                "the live token is wrong: the game that wrote this discovery file is not \
                 the one listening",
            );
            return (Response::line(request.id, Err(e)), true);
        }
        let op = match Op::parse(&request.op, request.args) {
            Ok(op) => op,
            Err(e) => return (Response::line(request.id, Err(WireError::usage(e))), false),
        };
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let incoming = Incoming {
            op,
            id: request.id,
            reply: reply_tx,
        };
        if self.tx.send(incoming).is_err() || self.stop.load(Ordering::SeqCst) {
            return (gone(request.id), true);
        }
        if let Some(wake) = &self.wake {
            wake();
        }
        // The game answers every request it takes, or drops it when it quits.
        match reply_rx.recv() {
            Ok(line) => (line, false),
            Err(_) => (gone(request.id), true),
        }
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

fn send(stream: &Stream, line: &str) -> io::Result<()> {
    let mut w = stream;
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
