//! The live socket: a local IPC endpoint the running game opens, and the client the
//! CLI's `--live` commands use.
//!
//! - **Endpoint.** `interprocess` local sockets: a named pipe `\\.\pipe\galaxy-<key>` on
//!   Windows, a Unix socket `<key>.sock` elsewhere. `<key>` is derived from the save
//!   file's path, so several games (one per save) can run at once.
//! - **Discovery.** While it runs, the game writes [`Discovery`] to `live.json` in the
//!   save's game-master folder (`<save>.gm/`) and a copy into a per-user registry
//!   folder, so `--live` finds it without being told the save.
//! - **Who may connect.** Only the local user: Windows named pipes reject remote clients
//!   and their default DACL lets only the owner (and SYSTEM and administrators) write;
//!   the Unix socket is mode 0600 in a folder only the user can open. On top of that
//!   every request carries the random token from the discovery file, which only the
//!   user can read.
//! - **Protocol.** Newline-delimited JSON. A request is
//!   `{"id": 1, "token": "...", "op": "state.get", "args": {...}}`; the reply is
//!   `{"id": 1, "ok": true, "result": ...}` or
//!   `{"id": 1, "ok": false, "error": {"code": 4, "kind": "bad_input", "message": "..."}}`.
//!   `watch` answers `ok` and then streams `{"stream": 1, "item": {...}}` lines until the
//!   client hangs up. One connection runs its requests in order.
//! - **Threads.** One I/O thread does all the socket I/O, non-blocking (named pipes
//!   have no I/O timeouts, so nothing may block on a client). It hands requests to the
//!   game through a channel ([`Server::poll`]); the game answers between frames and
//!   never waits on a client: replies go back through a bounded channel with
//!   `try_send`, and a stream that can't keep up drops items and says how many.
//! - **Limits** ([`Limits`]). A bounded number of connections; a request line is at
//!   most [`MAX_REQUEST`] bytes and must arrive whole within `read_stall`, the first
//!   one (with the token) within `read_stall` of connecting; a connection with nothing
//!   in flight that stays quiet for `idle` is closed; replies buffered for a client
//!   are capped at `max_out` bytes (every reply, errors included, waits for room, and
//!   a connection over it reads no further requests), and one that reads nothing for
//!   `write_stall` is dropped. Dropping the server closes every connection. Request arguments are
//!   checked ([`Op::check`]) before they reach the game, and a reply is at most
//!   [`MAX_REPLY`] bytes.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use galaxy_contract::history::HistoryQuery;
use galaxy_contract::ids::FactId;
use galaxy_contract::pack::ValidationReport;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::reads::{MAX_PATH, StateQuery};

/// Bump when requests or replies change incompatibly.
pub const PROTOCOL: u32 = 1;
/// Longest request line the game reads.
pub const MAX_REQUEST: u64 = 1024 * 1024;
/// Longest reply line the game sends; a longer one is answered with an error saying
/// to ask for less.
pub const MAX_REPLY: usize = 32 * 1024 * 1024;
/// Filters or fields in one `state.query`.
pub const MAX_QUERY_ITEMS: usize = 64;
/// Stream items buffered per watcher before the game starts dropping them.
pub const STREAM_BUFFER: usize = 1024;

/// What kind of local socket an endpoint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// A Windows named pipe; `name` is the pipe name without `\\.\pipe\`.
    NamedPipe,
    /// A Unix-domain socket; `name` is its path.
    UnixSocket,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub kind: EndpointKind,
    pub name: String,
}

impl Endpoint {
    /// A raw, blocking connection (what [`Client`] speaks over).
    pub fn connect(&self) -> io::Result<Stream> {
        Stream::connect(self.to_name()?)
    }

    fn to_name(&self) -> io::Result<Name<'_>> {
        match self.kind {
            EndpointKind::NamedPipe => {
                self.name
                    .as_str()
                    .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            }
            EndpointKind::UnixSocket => {
                self.name
                    .as_str()
                    .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            }
        }
    }
}

/// A short key for a save file: the first 16 hex digits of the BLAKE3 hash of its
/// absolute path. The live endpoint's name is built from it.
pub fn save_key(save: &Path) -> String {
    let path = std::fs::canonicalize(save)
        .or_else(|_| std::path::absolute(save))
        .unwrap_or_else(|_| save.to_path_buf());
    let hash = blake3::hash(path.to_string_lossy().as_bytes());
    hash.to_hex()[..16].to_owned()
}

/// What the running game writes so clients can find it. `live.json` in the save's
/// game-master folder, plus a copy in the per-user registry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    pub protocol: u32,
    pub endpoint: Endpoint,
    /// Sent with every request.
    pub token: String,
    pub pid: u32,
    pub save: PathBuf,
    /// Seconds since the Unix epoch when the game opened the socket (to pick the newest
    /// of several games). Wall-clock, but only the app reads it, never the sim.
    pub started: u64,
}

impl Discovery {
    pub fn read(path: &Path) -> Result<Discovery, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("no running game found at {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// One request's operation and arguments: `{"op": "...", "args": {...}}` on the wire.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum Op {
    /// Who is running: protocol, save, tick, pause and dock state.
    #[serde(rename = "hello")]
    Hello {},
    /// The world, or part of it (`stations.station.kettle_dock`, or slash-separated).
    #[serde(rename = "state.get")]
    StateGet {
        #[serde(default)]
        path: Option<String>,
    },
    /// Records of one store matching filters.
    #[serde(rename = "state.query")]
    StateQuery(StateQuery),
    /// History facts, oldest first.
    #[serde(rename = "history.query")]
    HistoryQuery(HistoryQuery),
    /// What changed since a fact, by place and person.
    #[serde(rename = "gm.digest")]
    Digest {
        #[serde(default)]
        since: Option<FactId>,
    },
    /// Applied, staged and quarantined packs.
    #[serde(rename = "gm.status")]
    Status {},
    /// Write the current state to a new save file at `to` (the sandbox's base).
    #[serde(rename = "gm.fork")]
    Fork { to: PathBuf },
    /// Stage a validated pack copy (already in the save's pack store) for the next calm
    /// moment.
    #[serde(rename = "gm.stage")]
    Stage {
        /// The copy's folder, relative to the game-master folder.
        dir: PathBuf,
        report: Box<ValidationReport>,
    },
    /// In-world rollback of a change: `cause.N` or `stage.N`.
    #[serde(rename = "gm.rollback")]
    Rollback { change: String },
    /// A PNG of what the player sees right now.
    #[serde(rename = "shot")]
    Shot { out: PathBuf },
    /// Stream facts, talk, game-master and calm-moment events as they happen.
    #[serde(rename = "watch")]
    Watch {
        #[serde(default)]
        since: Option<FactId>,
    },
}

impl Op {
    /// The wire name.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Hello {} => "hello",
            Op::StateGet { .. } => "state.get",
            Op::StateQuery(_) => "state.query",
            Op::HistoryQuery(_) => "history.query",
            Op::Digest { .. } => "gm.digest",
            Op::Status {} => "gm.status",
            Op::Fork { .. } => "gm.fork",
            Op::Stage { .. } => "gm.stage",
            Op::Rollback { .. } => "gm.rollback",
            Op::Shot { .. } => "shot",
            Op::Watch { .. } => "watch",
        }
    }

    /// Checks the arguments' sizes, so the game never spends a frame on an oversized
    /// one: paths, filters and fields up to [`MAX_PATH`] bytes, at most
    /// [`MAX_QUERY_ITEMS`] filters or fields.
    pub fn check(&self) -> Result<(), String> {
        let text = |what: &str, s: &str| {
            if s.len() > MAX_PATH {
                Err(format!(
                    "{what} is {} bytes; the most a request takes is {MAX_PATH}",
                    s.len()
                ))
            } else {
                Ok(())
            }
        };
        let file = |what: &str, p: &Path| text(what, &p.to_string_lossy());
        let list = |what: &str, items: &[String]| {
            if items.len() > MAX_QUERY_ITEMS {
                return Err(format!(
                    "{} {what}; the most a query takes is {MAX_QUERY_ITEMS}",
                    items.len()
                ));
            }
            items.iter().try_for_each(|i| text(what, i))
        };
        match self {
            Op::StateGet { path } => path.as_deref().map_or(Ok(()), |p| text("the path", p)),
            Op::StateQuery(q) => {
                text("the store", &q.store)?;
                list("filters", &q.filters)?;
                list("fields", &q.fields)
            }
            Op::Fork { to } => file("the fork's path", to),
            Op::Stage { dir, .. } => file("the copy's folder", dir),
            Op::Rollback { change } => text("the change", change),
            Op::Shot { out } => file("the shot's path", out),
            Op::Hello {}
            | Op::HistoryQuery(_)
            | Op::Digest { .. }
            | Op::Status {}
            | Op::Watch { .. } => Ok(()),
        }
    }

    /// Parses `op` with `args` (`null` reads as `{}`).
    pub fn parse(op: &str, args: Value) -> Result<Op, String> {
        let args = if args.is_null() { json!({}) } else { args };
        serde_json::from_value(json!({ "op": op, "args": args }))
            .map_err(|e| format!("bad `{op}` request: {e}"))
    }
}

/// A request line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub token: String,
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

/// A failed request, with the CLI's exit code and error kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: u8,
    pub kind: String,
    pub message: String,
    /// Printed instead of the error (a rejected pack's report).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl WireError {
    pub fn new(code: u8, kind: &str, message: impl Into<String>) -> Self {
        Self {
            code,
            kind: kind.to_owned(),
            message: message.into(),
            body: None,
        }
    }

    pub fn input(message: impl Into<String>) -> Self {
        Self::new(4, "bad_input", message)
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(1, "runtime", message)
    }
}

impl From<(u8, String)> for WireError {
    fn from((code, message): (u8, String)) -> Self {
        Self::new(
            code,
            if code == 4 { "bad_input" } else { "runtime" },
            message,
        )
    }
}

/// A reply line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

impl Response {
    /// The reply line; one over [`MAX_REPLY`] becomes an error.
    fn line(id: u64, result: Result<Value, WireError>) -> String {
        let line = Self::encode(id, result);
        if line.len() <= MAX_REPLY {
            return line;
        }
        Self::encode(
            id,
            Err(WireError::input(format!(
                "the reply is {} bytes, more than the {MAX_REPLY} the game sends; ask for less (a --path, --fields or --limit)",
                line.len()
            ))),
        )
    }

    fn encode(id: u64, result: Result<Value, WireError>) -> String {
        let r = match result {
            Ok(v) => Response {
                id,
                ok: true,
                result: Some(v),
                error: None,
            },
            Err(e) => Response {
                id,
                ok: false,
                result: None,
                error: Some(e),
            },
        };
        serde_json::to_string(&r).expect("responses serialize")
    }
}

/// A request waiting for the game. Answer it with [`Incoming::respond`] (from any
/// thread), or turn it into a stream with [`Incoming::into_stream`]. Dropping it
/// unanswered tells the client the game went away.
#[derive(Debug)]
pub struct Incoming {
    pub id: u64,
    pub op: Op,
    out: SyncSender<String>,
    gone: Arc<AtomicBool>,
}

impl Incoming {
    pub fn respond(self, result: Result<Value, WireError>) {
        let _ = self.out.try_send(Response::line(self.id, result));
    }

    /// Acknowledges a stream request and returns the sink for its items.
    pub fn into_stream(self, ack: Value) -> StreamSink {
        let _ = self.out.try_send(Response::line(self.id, Ok(ack)));
        StreamSink {
            id: self.id,
            out: self.out,
            gone: self.gone,
            dropped: 0,
        }
    }
}

/// Where one watcher's stream items go. Never blocks.
#[derive(Debug)]
pub struct StreamSink {
    id: u64,
    out: SyncSender<String>,
    /// Set by the I/O thread once the connection has closed.
    gone: Arc<AtomicBool>,
    dropped: u64,
}

impl StreamSink {
    /// Is the client still connected? The I/O thread notices a hang-up on its own,
    /// whether or not anything is being sent, so a sink can be dropped without one.
    pub fn is_open(&self) -> bool {
        !self.gone.load(Ordering::SeqCst)
    }

    /// Queues an item. Returns false once the client has gone (drop the sink). A full
    /// buffer drops the item and reports the count with the next one that fits.
    pub fn send(&mut self, item: &Value) -> bool {
        if self.dropped > 0 {
            let lag =
                json!({ "stream": self.id, "item": { "type": "lagged", "dropped": self.dropped } });
            match self.out.try_send(lag.to_string()) {
                Ok(()) => self.dropped = 0,
                Err(TrySendError::Full(_)) => {
                    self.dropped += 1;
                    return true;
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        let line = json!({ "stream": self.id, "item": item }).to_string();
        match self.out.try_send(line) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped += 1;
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

/// How long and how much a client may hold the socket. Wall-clock, in the I/O thread
/// only.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Connections open at once. Over that, a new one gets a `busy` error and is closed.
    pub max_connections: usize,
    /// A request line, once started, must arrive whole within this; so must the first
    /// one after connecting.
    pub read_stall: Duration,
    /// A connection with no request in flight that sends nothing for this long is
    /// closed.
    pub idle: Duration,
    /// Replies a client doesn't read for this long close its connection (a watcher's
    /// stream ends with it).
    pub write_stall: Duration,
    /// Reply bytes buffered per connection; beyond it the game's stream items wait in
    /// their bounded channel (and drop once that is full) and no further request is
    /// read, so error replies wait too. One reply may overshoot it.
    pub max_out: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 16,
            read_stall: Duration::from_secs(10),
            idle: Duration::from_secs(15 * 60),
            write_stall: Duration::from_secs(30),
            max_out: 4 * 1024 * 1024,
        }
    }
}

/// Where the server listens and what it advertises.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// The save the game is playing (the endpoint's name derives from it).
    pub save: PathBuf,
    /// The save's game-master folder: `live.json` goes here.
    pub gm_dir: PathBuf,
    /// The per-user registry folder (a copy of `live.json` named by the save key).
    pub registry: Option<PathBuf>,
    /// Overrides the endpoint (tests use a fresh name each). On macOS a Unix
    /// socket must be inside an existing directory with no group or other access.
    pub endpoint: Option<Endpoint>,
    pub limits: Limits,
}

/// The game's side of the socket. Dropping it stops the I/O thread, closes every
/// connection and removes the discovery files.
pub struct Server {
    rx: Receiver<Incoming>,
    discovery: Discovery,
    files: Vec<PathBuf>,
    stop: Arc<AtomicBool>,
    io: Option<JoinHandle<()>>,
    #[cfg(unix)]
    _socket_dir: Option<tempfile::TempDir>,
}

impl Server {
    /// Opens the endpoint, starts the I/O thread and writes the discovery files.
    pub fn start(opts: ServerOptions) -> io::Result<Server> {
        let key = save_key(&opts.save);
        std::fs::create_dir_all(&opts.gm_dir)?;
        #[cfg(unix)]
        let mut socket_dir = None;
        let endpoint = match opts.endpoint.clone() {
            Some(e) => e,
            None => default_endpoint(
                &key,
                &opts.gm_dir,
                #[cfg(unix)]
                &mut socket_dir,
            )?,
        };
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            // macOS ignores socket permissions for access checks and cannot fchmod
            // an unbound socket. The directory must protect it from the start,
            // including when the caller supplies an endpoint outside gm_dir.
            let parent = Path::new(&endpoint.name)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let metadata = std::fs::metadata(parent)?;
            if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the live socket needs a private parent directory (mode 0700)",
                ));
            }
        }
        let listener = {
            let name = endpoint.to_name()?;
            let options = ListenerOptions::new()
                .name(name)
                .nonblocking(ListenerNonblockingMode::Both)
                .try_overwrite(true);
            #[cfg(all(unix, not(target_os = "macos")))]
            let options = {
                use interprocess::os::unix::local_socket::ListenerOptionsExt;
                options.mode(0o600)
            };
            options.create_sync()?
        };
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&endpoint.name, std::fs::Permissions::from_mode(0o600))?;
        }
        let token = random_token()?;
        let discovery = Discovery {
            protocol: PROTOCOL,
            endpoint: endpoint.clone(),
            token: token.clone(),
            pid: std::process::id(),
            save: std::path::absolute(&opts.save).unwrap_or(opts.save.clone()),
            started: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        };
        let text = serde_json::to_string_pretty(&discovery).expect("discovery serializes");
        let mut files = vec![opts.gm_dir.join("live.json")];
        if let Some(reg) = &opts.registry {
            std::fs::create_dir_all(reg)?;
            files.push(reg.join(format!("{key}.json")));
        }
        for f in &files {
            write_private(f, &text)?;
        }

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let io = {
            let stop = stop.clone();
            let limits = opts.limits.clone();
            std::thread::Builder::new()
                .name("gm-io".into())
                .spawn(move || io_loop(listener, tx, token, stop, limits))?
        };
        Ok(Server {
            rx,
            discovery,
            files,
            stop,
            io: Some(io),
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
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
        // The I/O thread never blocks, so it sees the flag within one pass.
        if let Some(h) = self.io.take() {
            let _ = h.join();
        }
    }
}

fn default_endpoint(
    key: &str,
    gm_dir: &Path,
    #[cfg(unix)] socket_dir: &mut Option<tempfile::TempDir>,
) -> io::Result<Endpoint> {
    #[cfg(windows)]
    {
        let _ = gm_dir;
        Ok(Endpoint {
            kind: EndpointKind::NamedPipe,
            name: format!("galaxy-{key}"),
        })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Socket paths are short (about 100 bytes); fall back to a private temp folder.
        let mut path = gm_dir.join(format!("{key}.sock"));
        if path.as_os_str().len() > 100 {
            let dir = tempfile::Builder::new()
                .prefix("galaxy-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()?;
            path = dir.path().join(format!("{key}.sock"));
            *socket_dir = Some(dir);
        } else {
            std::fs::set_permissions(gm_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Endpoint {
            kind: EndpointKind::UnixSocket,
            name: path.to_string_lossy().into_owned(),
        })
    }
}

/// 32 random bytes as hex, from the OS.
fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes a file only the user can read (on Unix; Windows user folders already are).
fn write_private(path: &Path, text: &str) -> io::Result<()> {
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

fn token_matches(given: &str, want: &str) -> bool {
    // Same length, then every byte: no early exit on the first difference.
    given.len() == want.len()
        && given
            .bytes()
            .zip(want.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// How long the I/O thread rests when a pass found nothing to do.
const IO_REST: Duration = Duration::from_millis(3);
/// New connections taken per pass.
const ACCEPTS_PER_PASS: usize = 8;
/// After a closing error reply is written, how long the client gets to read it and hang
/// up before the connection is closed anyway.
const LINGER: Duration = Duration::from_secs(1);

/// Is this "not now" on a non-blocking stream? (A Windows pipe in no-wait mode reads
/// "no data" as zero bytes and takes nothing when full as a zero-byte write instead;
/// see [`Conn::pump`].)
fn would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}

/// Closes a stream without waiting for the client to read what's left. (On Windows,
/// dropping a pipe stream with unflushed data hands it to a thread that flushes it,
/// which blocks for as long as the client doesn't read.)
fn close(stream: Stream) {
    #[cfg(windows)]
    {
        let Stream::NamedPipe(s) = stream;
        drop(std::os::windows::io::OwnedHandle::from(s));
    }
    #[cfg(not(windows))]
    drop(stream);
}

/// A request the game hasn't finished answering: one reply, or a stream that lasts
/// until the game drops its sink.
struct InFlight {
    id: u64,
    rx: Receiver<String>,
    answered: bool,
    /// Shared with the game's side; set when this ends (see [`StreamSink::is_open`]).
    gone: Arc<AtomicBool>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.gone.store(true, Ordering::SeqCst);
    }
}

/// One client connection, driven by [`Conn::pump`].
struct Conn {
    stream: Option<Stream>,
    input: Vec<u8>,
    /// Bytes not yet written; written ones leave the front.
    out: VecDeque<u8>,
    authorized: bool,
    opened: Instant,
    /// Last time a request line arrived or a request finished.
    active: Instant,
    /// When the unfinished line in `input` started arriving.
    partial: Option<Instant>,
    /// Last time a write made progress (or output started waiting).
    wrote: Instant,
    inflight: Option<InFlight>,
    /// Closing after `out` is written (an error the client must see); since when.
    closing: Option<Instant>,
    eof: bool,
    dead: bool,
}

impl Conn {
    fn new(stream: Stream, now: Instant) -> Conn {
        Conn {
            stream: Some(stream),
            input: Vec::new(),
            out: VecDeque::new(),
            authorized: false,
            opened: now,
            active: now,
            partial: None,
            wrote: now,
            inflight: None,
            closing: None,
            eof: false,
            dead: false,
        }
    }

    /// Is there room for another reply? Every reply waits for it; see [`Limits::max_out`].
    fn has_room(&self, limits: &Limits) -> bool {
        self.out.len() < limits.max_out
    }

    fn queue(&mut self, line: &str, now: Instant) {
        if self.out.is_empty() {
            self.wrote = now;
        }
        self.out.extend(line.as_bytes());
        self.out.push_back(b'\n');
    }

    fn close_after(&mut self, line: &str, now: Instant) {
        self.queue(line, now);
        self.closing = Some(now);
    }

    /// One pass: read what's there, hand a whole line to the game, collect its
    /// replies, write what fits, and enforce the deadlines. Returns whether anything
    /// moved.
    fn pump(&mut self, tx: &Sender<Incoming>, token: &str, limits: &Limits, now: Instant) -> bool {
        let mut moved = false;
        let Some(stream) = self.stream.as_mut() else {
            self.dead = true;
            return false;
        };
        // Read, up to one request line's worth ahead.
        let mut buf = [0u8; 16 * 1024];
        while !self.eof && (self.input.len() as u64) <= MAX_REQUEST {
            match stream.read(&mut buf) {
                // A Windows pipe in no-wait mode reads "no data" as zero bytes too: a
                // zero-byte write tells them apart (it fails once the client is gone).
                Ok(0) if cfg!(windows) => {
                    if stream.write(&[]).is_err() {
                        self.eof = true;
                    }
                    break;
                }
                Ok(0) => self.eof = true,
                Ok(n) => {
                    self.input.extend_from_slice(&buf[..n]);
                    moved = true;
                }
                Err(e) if would_block(&e) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.dead = true;
                    return true;
                }
            }
        }
        // Requests, one at a time, while their replies have room.
        while self.inflight.is_none() && self.closing.is_none() && self.has_room(limits) {
            let end = self.input.iter().position(|b| *b == b'\n');
            if end.unwrap_or(self.input.len()) as u64 > MAX_REQUEST {
                let msg = Response::line(0, Err(WireError::input("request line too long")));
                self.close_after(&msg, now);
                break;
            }
            let Some(end) = end else {
                break;
            };
            let line: Vec<u8> = self.input.drain(..=end).collect();
            self.active = now;
            moved = true;
            self.request(&line, tx, token, now);
            if self.dead {
                return true;
            }
        }
        // Only an unfinished line is partial; a whole one waiting behind a request in
        // flight isn't.
        self.partial = if self.input.is_empty() || self.input.contains(&b'\n') {
            None
        } else {
            self.partial.or(Some(now))
        };
        // Replies, while the output buffer has room.
        while self.has_room(limits)
            && let Some(f) = &mut self.inflight
        {
            match f.rx.try_recv() {
                Ok(msg) => {
                    f.answered = true;
                    self.queue(&msg, now);
                    moved = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let (id, answered) = (f.id, f.answered);
                    self.inflight = None;
                    self.active = now;
                    if !answered {
                        let msg = Response::line(
                            id,
                            Err(WireError::runtime("the game dropped the request")),
                        );
                        self.queue(&msg, now);
                    }
                    moved = true;
                }
            }
        }
        // Write what the pipe takes.
        while !self.out.is_empty() {
            let Some(stream) = self.stream.as_mut() else {
                break;
            };
            match stream.write(self.out.as_slices().0) {
                // A full no-wait pipe takes nothing and says so with zero bytes.
                Ok(0) => break,
                Ok(n) => {
                    self.out.drain(..n);
                    self.wrote = now;
                    moved = true;
                }
                Err(e) if would_block(&e) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.dead = true;
                    return true;
                }
            }
        }
        // A big reply's buffer isn't kept once it's out.
        if self.out.is_empty() && self.out.capacity() > limits.max_out {
            self.out = VecDeque::new();
        }
        // Deadlines.
        let flushed = self.out.is_empty();
        let late = |since: Instant, limit: Duration| now.duration_since(since) > limit;
        self.dead = (!flushed && late(self.wrote, limits.write_stall))
            || self.partial.is_some_and(|p| late(p, limits.read_stall))
            || (!self.authorized && self.closing.is_none() && late(self.opened, limits.read_stall))
            || (self.inflight.is_none() && flushed && late(self.active, limits.idle))
            || self.closing.is_some_and(|c| flushed && (self.eof || late(c, LINGER)))
            // The client hung up: nothing it asked for is worth finishing.
            || (self.eof && self.closing.is_none());
        moved
    }

    /// Handles one request line.
    fn request(&mut self, line: &[u8], tx: &Sender<Incoming>, token: &str, now: Instant) {
        let text = String::from_utf8_lossy(line);
        if text.trim().is_empty() {
            return;
        }
        let req: Request = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                let msg = Response::line(0, Err(WireError::input(format!("bad request: {e}"))));
                if self.authorized {
                    self.queue(&msg, now);
                } else {
                    self.close_after(&msg, now);
                }
                return;
            }
        };
        if !token_matches(&req.token, token) {
            let msg = Response::line(
                req.id,
                Err(WireError::new(2, "unauthorized", "wrong or missing token")),
            );
            self.close_after(&msg, now);
            return;
        }
        self.authorized = true;
        let op = match Op::parse(&req.op, req.args).and_then(|op| op.check().map(|()| op)) {
            Ok(op) => op,
            Err(e) => {
                self.queue(&Response::line(req.id, Err(WireError::input(e))), now);
                return;
            }
        };
        let (out, rx) = mpsc::sync_channel(STREAM_BUFFER);
        let gone = Arc::new(AtomicBool::new(false));
        if tx
            .send(Incoming {
                id: req.id,
                op,
                out,
                gone: gone.clone(),
            })
            .is_err()
        {
            self.dead = true;
            return;
        }
        self.inflight = Some(InFlight {
            id: req.id,
            rx,
            answered: false,
            gone,
        });
    }
}

fn io_loop(
    listener: interprocess::local_socket::Listener,
    tx: Sender<Incoming>,
    token: String,
    stop: Arc<AtomicBool>,
    limits: Limits,
) {
    let mut conns: Vec<Conn> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        let mut moved = false;
        for _ in 0..ACCEPTS_PER_PASS {
            let stream = match listener.accept() {
                Ok(s) => s,
                Err(_) => break,
            };
            moved = true;
            if stream.set_nonblocking(true).is_err() {
                close(stream);
                continue;
            }
            let mut conn = Conn::new(stream, now);
            let open = conns.iter().filter(|c| c.closing.is_none()).count();
            if open >= limits.max_connections {
                // Turned away, with a reason if the client reads it in time. Closing
                // connections are capped too, so a flood can't pile them up.
                if conns.len() >= limits.max_connections * 2 {
                    close(conn.stream.take().expect("just opened"));
                    continue;
                }
                let msg = Response::line(
                    0,
                    Err(WireError::new(
                        1,
                        "busy",
                        format!(
                            "{} clients are already connected to the game",
                            limits.max_connections
                        ),
                    )),
                );
                conn.close_after(&msg, now);
            }
            conns.push(conn);
        }
        for c in &mut conns {
            moved |= c.pump(&tx, &token, &limits, now);
        }
        conns.retain_mut(|c| {
            if c.dead
                && let Some(s) = c.stream.take()
            {
                close(s);
            }
            !c.dead
        });
        if !moved {
            std::thread::sleep(IO_REST);
        }
    }
    for c in &mut conns {
        if let Some(s) = c.stream.take() {
            close(s);
        }
    }
}

fn write_line(send: &mut Stream, line: &str) -> io::Result<()> {
    send.write_all(line.as_bytes())?;
    send.write_all(b"\n")?;
    send.flush()
}

/// A failed call: the connection, or the game said no.
#[derive(Debug)]
pub enum CallError {
    Io(io::Error),
    Remote(WireError),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Io(e) => write!(f, "the live game: {e}"),
            CallError::Remote(e) => write!(f, "{}", e.message),
        }
    }
}

impl From<io::Error> for CallError {
    fn from(e: io::Error) -> Self {
        CallError::Io(e)
    }
}

/// The CLI's side.
pub struct Client {
    stream: BufReader<Stream>,
    token: String,
    next: u64,
    pub discovery: Discovery,
}

impl Client {
    /// Connects to the game a discovery file describes. Calls time out after
    /// `timeout` (none for streams) where the platform can (Unix sockets; named pipes
    /// have no I/O timeouts).
    pub fn connect(discovery: &Discovery, timeout: Option<Duration>) -> io::Result<Client> {
        if discovery.protocol != PROTOCOL {
            return Err(io::Error::other(format!(
                "the running game speaks protocol {}, this CLI {PROTOCOL}",
                discovery.protocol
            )));
        }
        let stream = discovery.endpoint.connect()?;
        let _ = stream.set_recv_timeout(timeout);
        Ok(Client {
            stream: BufReader::new(stream),
            token: discovery.token.clone(),
            next: 1,
            discovery: discovery.clone(),
        })
    }

    fn send(&mut self, op: &str, args: Value) -> Result<u64, CallError> {
        let id = self.next;
        self.next += 1;
        let req = Request {
            id,
            token: self.token.clone(),
            op: op.to_owned(),
            args,
        };
        let line = serde_json::to_string(&req).expect("requests serialize");
        write_line(self.stream.get_mut(), &line)?;
        Ok(id)
    }

    fn read(&mut self) -> Result<Value, CallError> {
        let mut line = String::new();
        if self.stream.read_line(&mut line)? == 0 {
            return Err(CallError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the game closed the connection",
            )));
        }
        serde_json::from_str(&line).map_err(|e| CallError::Io(io::Error::other(e)))
    }

    fn reply(&mut self, id: u64) -> Result<Value, CallError> {
        loop {
            let v = self.read()?;
            if v.get("stream").is_some() {
                continue;
            }
            let r: Response =
                serde_json::from_value(v).map_err(|e| CallError::Io(io::Error::other(e)))?;
            if r.id != id && r.id != 0 {
                continue;
            }
            return match (r.ok, r.result, r.error) {
                (true, result, _) => Ok(result.unwrap_or(Value::Null)),
                (false, _, Some(e)) => Err(CallError::Remote(e)),
                (false, _, None) => Err(CallError::Remote(WireError::runtime("request failed"))),
            };
        }
    }

    /// One request and its reply.
    pub fn call(&mut self, op: &str, args: Value) -> Result<Value, CallError> {
        let id = self.send(op, args)?;
        self.reply(id)
    }

    /// Starts a stream (`watch`): returns the acknowledgement; read items with
    /// [`Client::next_item`].
    pub fn stream(&mut self, op: &str, args: Value) -> Result<Value, CallError> {
        self.call(op, args)
    }

    /// The next stream item.
    pub fn next_item(&mut self) -> Result<Value, CallError> {
        loop {
            let v = self.read()?;
            if let Some(item) = v.get("item") {
                return Ok(item.clone());
            }
        }
    }
}

/// Finds a running game: the one playing `save` (its `live.json`), else the newest one
/// in `registry` that answers.
pub fn find(save: Option<&Path>, registry: Option<&Path>) -> Result<Discovery, String> {
    if let Some(save) = save {
        let path = crate::store::GmStore::for_save(save)
            .dir()
            .join("live.json");
        let d = Discovery::read(&path)?;
        return Ok(d);
    }
    let Some(registry) = registry else {
        return Err("no running game found: pass --live <save>".into());
    };
    let mut found: Vec<Discovery> = std::fs::read_dir(registry)
        .map_err(|_| "no running game found (nothing is registered)".to_owned())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| Discovery::read(&p).ok())
        .collect();
    found.sort_by_key(|d| std::cmp::Reverse(d.started));
    found
        .into_iter()
        .find(|d| answers(d, Duration::from_secs(5)))
        .ok_or_else(|| "no running game answered; is `galaxy play` open?".to_owned())
}

/// Does the game a discovery file names answer `hello` within `wait`? Probed on a
/// thread, so a hung game can't hang the caller.
pub fn answers(d: &Discovery, wait: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    let d = d.clone();
    let probe = std::thread::Builder::new()
        .name("gm-probe".into())
        .spawn(move || {
            let ok = Client::connect(&d, Some(wait))
                .ok()
                .is_some_and(|mut c| c.call("hello", json!({})).is_ok());
            let _ = tx.send(ok);
        });
    probe.is_ok() && rx.recv_timeout(wait).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    const LIMITS: Limits = Limits {
        max_connections: 4,
        read_stall: Duration::from_secs(60),
        idle: Duration::from_secs(60),
        write_stall: Duration::from_secs(60),
        max_out: 64 * 1024,
    };

    /// A server-side connection and the client's end of it.
    fn pair(dir: &Path) -> (Conn, Stream) {
        let endpoint = if cfg!(windows) {
            Endpoint {
                kind: EndpointKind::NamedPipe,
                name: format!("galaxy-unit-{}-{}", save_key(dir), std::process::id()),
            }
        } else {
            Endpoint {
                kind: EndpointKind::UnixSocket,
                name: dir.join("u.sock").to_string_lossy().into_owned(),
            }
        };
        let listener = ListenerOptions::new()
            .name(endpoint.to_name().unwrap())
            .nonblocking(ListenerNonblockingMode::Both)
            .create_sync()
            .unwrap();
        let client = endpoint.connect().unwrap();
        let started = Instant::now();
        let stream = loop {
            match listener.accept() {
                Ok(s) => break s,
                Err(_) => {
                    assert!(started.elapsed() < Duration::from_secs(10), "no accept");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        stream.set_nonblocking(true).unwrap();
        (Conn::new(stream, Instant::now()), client)
    }

    #[test]
    fn a_slow_reader_doesnt_grow_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let (mut conn, mut client) = pair(dir.path());
        let (tx, rx) = mpsc::channel();
        client
            .write_all(b"{\"id\": 1, \"token\": \"t\", \"op\": \"watch\", \"args\": {}}\n")
            .unwrap();
        let started = Instant::now();
        let req = loop {
            conn.pump(&tx, "t", &LIMITS, Instant::now());
            if let Ok(r) = rx.try_recv() {
                break r;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no request");
        };
        let mut sink = req.into_stream(json!({}));
        // The client reads steadily, a little at a time, slower than the game sends.
        let read = Arc::new(AtomicUsize::new(0));
        let reader = {
            let read = read.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                while let Ok(n @ 1..) = client.read(&mut buf) {
                    read.fetch_add(n, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_micros(200));
                }
            })
        };
        let item = json!({ "pad": "x".repeat(4096) });
        let line = 4096 + 64;
        let (mut most, mut capacity) = (0, 0);
        let started = Instant::now();
        while read.load(Ordering::SeqCst) < 32 * LIMITS.max_out {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "the reader stalled"
            );
            sink.send(&item);
            conn.pump(&tx, "t", &LIMITS, Instant::now());
            assert!(!conn.dead);
            most = most.max(conn.out.len());
            capacity = capacity.max(conn.out.capacity());
        }
        assert!(most <= LIMITS.max_out + line, "{most} bytes buffered");
        assert!(capacity <= 4 * LIMITS.max_out, "{capacity} bytes allocated");
        close(conn.stream.take().unwrap());
        reader.join().unwrap();
    }

    #[test]
    fn error_replies_wait_for_room_too() {
        let dir = tempfile::tempdir().unwrap();
        let (mut conn, mut client) = pair(dir.path());
        let (tx, _rx) = mpsc::channel();
        // Authorized requests the I/O thread refuses itself, from a client that never
        // reads the replies.
        let writer = std::thread::spawn(move || {
            let line = b"{\"id\": 1, \"token\": \"t\", \"op\": \"nope\", \"args\": {}}\n";
            let chunk: Vec<u8> = line.iter().copied().cycle().take(16 * 1024).collect();
            for _ in 0..256 {
                if client.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
        let (mut most, mut input) = (0, 0);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            conn.pump(&tx, "t", &LIMITS, Instant::now());
            assert!(!conn.dead);
            most = most.max(conn.out.len());
            input = input.max(conn.input.len());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            most >= LIMITS.max_out / 2,
            "the replies never backed up: {most}"
        );
        assert!(most <= LIMITS.max_out + 256, "{most} bytes buffered");
        assert!(
            input as u64 <= MAX_REQUEST + 16 * 1024,
            "{input} bytes read ahead"
        );
        close(conn.stream.take().unwrap());
        writer.join().unwrap();
    }
}
