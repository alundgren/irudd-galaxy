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
//! - **Threads.** An accept thread and one thread per connection do all the socket I/O.
//!   They hand requests to the game through a channel ([`Server::poll`]); the game
//!   answers between frames and never waits on a client: replies go back through a
//!   bounded channel with `try_send`, and a stream that can't keep up drops items and
//!   says how many.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use galaxy_contract::history::HistoryQuery;
use galaxy_contract::ids::FactId;
use galaxy_contract::pack::ValidationReport;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::reads::StateQuery;

/// Bump when requests or replies change incompatibly.
pub const PROTOCOL: u32 = 1;
/// Longest request line the game reads.
pub const MAX_REQUEST: u64 = 1024 * 1024;
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
    fn line(id: u64, result: Result<Value, WireError>) -> String {
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
            dropped: 0,
        }
    }
}

/// Where one watcher's stream items go. Never blocks.
#[derive(Debug)]
pub struct StreamSink {
    id: u64,
    out: SyncSender<String>,
    dropped: u64,
}

impl StreamSink {
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

/// Where the server listens and what it advertises.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// The save the game is playing (the endpoint's name derives from it).
    pub save: PathBuf,
    /// The save's game-master folder: `live.json` goes here.
    pub gm_dir: PathBuf,
    /// The per-user registry folder (a copy of `live.json` named by the save key).
    pub registry: Option<PathBuf>,
    /// Overrides the endpoint (tests use a fresh name each).
    pub endpoint: Option<Endpoint>,
}

/// The game's side of the socket. Dropping it stops listening and removes the
/// discovery files.
pub struct Server {
    rx: Receiver<Incoming>,
    discovery: Discovery,
    files: Vec<PathBuf>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    #[cfg(unix)]
    _socket_dir: Option<tempfile::TempDir>,
}

impl Server {
    /// Opens the endpoint, starts the accept thread and writes the discovery files.
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
        let listener = {
            let name = endpoint.to_name()?;
            let options = ListenerOptions::new()
                .name(name)
                .nonblocking(ListenerNonblockingMode::Neither)
                .try_overwrite(true);
            #[cfg(unix)]
            let options = {
                use interprocess::os::unix::local_socket::ListenerOptionsExt;
                options.mode(0o600)
            };
            options.create_sync()?
        };
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
        let token = Arc::new(token);
        let accept = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("gm-accept".into())
                .spawn(move || accept_loop(listener, tx, token, stop))?
        };
        Ok(Server {
            rx,
            discovery,
            files,
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
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
        // Wake the accept thread so it sees the flag.
        if let Ok(name) = self.discovery.endpoint.to_name() {
            let _ = Stream::connect(name);
        }
        if let Some(h) = self.accept.take() {
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
            let dir = tempfile::Builder::new().prefix("galaxy-").tempdir()?;
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

fn accept_loop(
    listener: interprocess::local_socket::Listener,
    tx: Sender<Incoming>,
    token: Arc<String>,
    stop: Arc<AtomicBool>,
) {
    loop {
        let conn = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match conn {
            Ok(stream) => {
                let tx = tx.clone();
                let token = token.clone();
                let _ = std::thread::Builder::new()
                    .name("gm-conn".into())
                    .spawn(move || serve_connection(stream, tx, token));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
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

fn serve_connection(stream: Stream, tx: Sender<Incoming>, token: Arc<String>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.by_ref().take(MAX_REQUEST + 1).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.len() as u64 > MAX_REQUEST {
            let _ = write_line(
                reader.get_mut(),
                &Response::line(0, Err(WireError::input("request line too long"))),
            );
            return;
        }
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let msg = Response::line(0, Err(WireError::input(format!("bad request: {e}"))));
                if write_line(reader.get_mut(), &msg).is_err() {
                    return;
                }
                continue;
            }
        };
        if !token_matches(&req.token, &token) {
            let msg = Response::line(
                req.id,
                Err(WireError::new(2, "unauthorized", "wrong or missing token")),
            );
            let _ = write_line(reader.get_mut(), &msg);
            return;
        }
        let op = match Op::parse(&req.op, req.args) {
            Ok(op) => op,
            Err(e) => {
                if write_line(
                    reader.get_mut(),
                    &Response::line(req.id, Err(WireError::input(e))),
                )
                .is_err()
                {
                    return;
                }
                continue;
            }
        };
        let (out, rx) = mpsc::sync_channel(STREAM_BUFFER);
        if tx
            .send(Incoming {
                id: req.id,
                op,
                out,
            })
            .is_err()
        {
            return;
        }
        let mut answered = false;
        // Everything the game sends for this request: one reply, or a stream that ends
        // when the game drops its sink.
        while let Ok(msg) = rx.recv() {
            answered = true;
            if write_line(reader.get_mut(), &msg).is_err() {
                return;
            }
        }
        if !answered {
            let msg = Response::line(
                req.id,
                Err(WireError::runtime("the game dropped the request")),
            );
            if write_line(reader.get_mut(), &msg).is_err() {
                return;
            }
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
        let stream = Stream::connect(discovery.endpoint.to_name()?)?;
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
