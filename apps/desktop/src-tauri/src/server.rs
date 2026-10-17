//! Local server (SPEC.md §8.1, §11). Binds 127.0.0.1 only, requires the token
//! on every request, answers hooks with `200 {}` at once and processes them
//! afterwards on a single worker, in arrival order. On a session's first
//! request, and on every `UserPromptSubmit`, it records which process sent it
//! (§7.2 step 2, §9.4, ADR 0014).

use crate::jump::{self, ProcessId};
use crate::log::utc;
use crate::platform::{Platform, fill_random};
use crate::sessions::Sessions;
use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc;

/// Over this, the event is dropped but the reply is still `200 {}` (ADR 0011).
pub const BODY_LIMIT: usize = 16 * 1024 * 1024;
pub const PORTS: RangeInclusive<u16> = 49152..=65535;
const TOKEN_HEADER: &str = "x-knock-token";
/// `X-Knock-Sim: 1` marks a request from `knock sim` (§8.1).
const SIM_HEADER: &str = "x-knock-sim";

/// `%APPDATA%\Knock\server.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub token: String,
}

impl ServerConfig {
    fn generate() -> io::Result<Self> {
        let mut b = [0u8; 34];
        fill_random(&mut b)?;
        // The range holds exactly 2^14 ports, so the modulo is unbiased.
        let span = PORTS.end() - PORTS.start() + 1;
        let port = PORTS.start() + u16::from_le_bytes([b[32], b[33]]) % span;
        Ok(Self {
            port,
            token: base64url(&b[..32]),
        })
    }
}

/// Loads `server.json` from `dir` and binds its port. On first run (no file)
/// picks a free port and a new token, then saves them; after that the port is
/// fixed. The Repair flow for a port that can't bind later is not built yet.
pub fn listen(dir: &Path) -> io::Result<(ServerConfig, TcpListener)> {
    let path = dir.join("server.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let cfg = parse_config(&bytes)?;
            let listener = bind(cfg.port)?;
            Ok((cfg, listener))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            for _ in 0..10 {
                let cfg = ServerConfig::generate()?;
                if let Ok(listener) = bind(cfg.port) {
                    std::fs::create_dir_all(dir)?;
                    // Durable: temp file, flush to disk, then rename.
                    let tmp = dir.join("server.json.tmp");
                    let mut f = std::fs::File::create(&tmp)?;
                    f.write_all(&serde_json::to_vec_pretty(&cfg)?)?;
                    f.sync_all()?;
                    drop(f);
                    std::fs::rename(&tmp, &path)?;
                    return Ok((cfg, listener));
                }
            }
            Err(io::Error::other("no free port found"))
        }
        Err(e) => Err(e),
    }
}

/// Parses `server.json` and rejects anything `generate` could not have made:
/// a port outside 49152-65535 or a token that isn't 43 base64url characters
/// (32 bytes, §8.1).
fn parse_config(bytes: &[u8]) -> io::Result<ServerConfig> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_owned());
    let cfg: ServerConfig =
        serde_json::from_slice(bytes).map_err(|_| bad("server.json is not valid"))?;
    if !PORTS.contains(&cfg.port) {
        return Err(bad("server.json port out of range"));
    }
    let base64url = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    if cfg.token.len() != 43 || !cfg.token.bytes().all(base64url) {
        return Err(bad("server.json token is not 43 base64url characters"));
    }
    Ok(cfg)
}

fn bind(port: u16) -> io::Result<TcpListener> {
    // Loopback only, never 0.0.0.0 (§8.1, §11.1).
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}

/// Starts the server on Tauri's async runtime. Called once by the app.
pub fn start(platform: Arc<dyn Platform>, sessions: Arc<Sessions>) -> io::Result<()> {
    let base = std::env::var_os("APPDATA").ok_or_else(|| io::Error::other("APPDATA not set"))?;
    let (cfg, listener) = listen(&PathBuf::from(base).join("Knock"))?;
    listener.set_nonblocking(true)?;
    tracing::info!(port = cfg.port, "server listening on 127.0.0.1");
    let opts = Options {
        platform: Some(platform),
        sessions,
        ..Options::from_env()
    };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = serve(listener, cfg.token, opts).await {
            tracing::error!(kind = ?e.kind(), "server stopped");
        }
    });
    Ok(())
}

/// What the server works with. Tests leave `platform` out, so no process
/// lookups happen; the rest are debug-build knobs for Phase 0 tests.
#[derive(Default)]
pub struct Options {
    /// For process and window lookups (§9.4).
    pub platform: Option<Arc<dyn Platform>>,
    pub sessions: Arc<Sessions>,
    /// Sleep this long before replying, to test a slow Knock (V-2).
    pub delay: Duration,
    #[cfg(debug_assertions)]
    pub recorder: Option<recorder::Recorder>,
}

impl Options {
    fn from_env() -> Self {
        #[cfg(debug_assertions)]
        {
            let delay = std::env::var("KNOCK_DELAY_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis)
                .unwrap_or_default();
            if !delay.is_zero() {
                tracing::warn!(delay_ms = delay.as_millis(), "debug reply delay is on");
            }
            Self {
                delay,
                recorder: recorder::from_env(),
                ..Self::default()
            }
        }
        #[cfg(not(debug_assertions))]
        Self::default()
    }
}

struct Shared {
    token: String,
    /// The port clients connect to, for the connection-owner lookup.
    port: u16,
    platform: Option<Arc<dyn Platform>>,
    sessions: Arc<Sessions>,
    started: Instant,
    delay: Duration,
    stats: Mutex<Stats>,
    /// Locked so `at` is taken and the event queued in one step: queue order
    /// then always matches timestamp order.
    tx: Mutex<mpsc::UnboundedSender<Received>>,
}

#[derive(Default)]
struct Stats {
    last_event_at: Option<SystemTime>,
}

struct Received {
    /// Time the event was queued, after its body finished arriving. Never
    /// decreases in queue order. Read only by the Phase 0 recorder for now.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    at: Instant,
    reply: Duration,
    /// Raw body, read only by the Phase 0 recorder.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    body: Bytes,
    head: Option<Head>,
    lookup: Option<Lookup>,
}

/// A connection-owner lookup made before the reply.
struct Lookup {
    process: Option<ProcessId>,
    took: Duration,
}

pub async fn serve(listener: TcpListener, token: String, mut opts: Options) -> io::Result<()> {
    let port = listener.local_addr()?.port();
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let (tx, rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared {
        token,
        port,
        platform: opts.platform.take(),
        sessions: Arc::clone(&opts.sessions),
        started: Instant::now(),
        delay: opts.delay,
        stats: Mutex::default(),
        tx: Mutex::new(tx),
    });
    tokio::spawn(worker(Arc::clone(&shared), rx, opts));
    let app = Router::new()
        .route("/v1/hooks/claude-code", post(hook))
        .route("/v1/health", get(health))
        // Token check before routing, so unknown paths and wrong methods get
        // 401 too (§11.1).
        .layer(from_fn_with_state(Arc::clone(&shared), require_token))
        .with_state(shared);
    // Peer addresses give the client port for the connection-owner lookup.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

fn authorized(token: &str, headers: &HeaderMap) -> bool {
    headers.get(TOKEN_HEADER).is_some_and(|v| {
        let (a, b) = (v.as_bytes(), token.as_bytes());
        // Constant-time compare.
        a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
    })
}

async fn require_token(State(s): State<Arc<Shared>>, req: Request, next: Next) -> Response {
    if !authorized(&s.token, req.headers()) {
        // Never log the body, the path or the header value.
        tracing::warn!("request rejected: missing or wrong token");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(req).await
}

fn json(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

async fn hook(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let start = Instant::now();
    let sim = req.headers().get(SIM_HEADER).is_some_and(|v| v == "1");
    // Queue before any further await: if the client gives up, the event is
    // still processed. `reply` is the time to here (§6: under 100 ms).
    match read_capped(req.into_body()).await {
        Ok(Some(body)) => {
            let head = serde_json::from_slice::<Head>(&body).ok();
            // A simulated session has no real process: no lookup, so its
            // precision stays `none` (§8.1).
            let lookup = head
                .as_ref()
                .filter(|_| !sim)
                .and_then(|h| lookup(&s, h, peer.port()));
            enqueue(&s, start, body, head, lookup);
        }
        // Never an error status for a request with a valid token (ADR 0011).
        _ => tracing::warn!("hook dropped: body over 16 MB or unreadable"),
    }
    if !s.delay.is_zero() {
        tokio::time::sleep(s.delay).await;
    }
    // Never any decision fields (§11.2).
    json("{}".into())
}

/// Reads the whole body but keeps at most `BODY_LIMIT` bytes (plus one
/// in-flight frame). Over the limit the rest is read and discarded, then
/// `None`: replying before the upload ends would reset the client's
/// connection on Windows.
async fn read_capped(mut body: Body) -> Result<Option<Bytes>, axum::Error> {
    let mut buf = Vec::new();
    let mut over = false;
    while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let Ok(data) = frame?.into_data() else {
            continue; // trailers
        };
        if !over && buf.len() + data.len() <= BODY_LIMIT {
            buf.extend_from_slice(&data);
        } else if !over {
            over = true;
            buf = Vec::new();
        }
    }
    Ok((!over).then(|| buf.into()))
}

/// §9.4 step 1, until a session's process is known and again on every
/// `UserPromptSubmit` (ADR 0014). Runs before the reply: once the client
/// closes, its end of the connection no longer has an owner.
fn lookup(s: &Shared, head: &Head, client_port: u16) -> Option<Lookup> {
    let platform = s.platform.as_ref()?;
    if head.hook_event_name != "UserPromptSubmit" && s.sessions.process(&head.session_id).is_some()
    {
        return None;
    }
    let t = Instant::now();
    let process = platform.connection_owner(client_port, s.port);
    Some(Lookup {
        process,
        took: t.elapsed(),
    })
}

fn enqueue(s: &Shared, start: Instant, body: Bytes, head: Option<Head>, lookup: Option<Lookup>) {
    let tx = s.tx.lock().unwrap_or_else(|e| e.into_inner());
    let at = Instant::now();
    let _ = tx.send(Received {
        at,
        reply: at - start,
        body,
        head,
        lookup,
    });
}

async fn health(State(s): State<Arc<Shared>>) -> Response {
    let stats = s.stats.lock().unwrap_or_else(|e| e.into_inner());
    json(
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptimeS": s.started.elapsed().as_secs(),
            "sessionCount": s.sessions.len(),
            "lastEventAt": stats.last_event_at.map(utc),
        })
        .to_string(),
    )
}

/// The only fields read from a payload in this spike. Everything else,
/// including all free text, is skipped by the parser and never kept. `cwd` is
/// reduced to `project` and never stored or logged (§11.4, §11.5).
#[derive(Deserialize)]
struct Head {
    session_id: String,
    hook_event_name: String,
    cwd: Option<String>,
    /// Read only by the Phase 0 toast (V-12).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    tool_name: Option<String>,
}

#[cfg_attr(not(debug_assertions), allow(unused_mut, unused_variables))]
async fn worker(s: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<Received>, mut opts: Options) {
    while let Some(r) = rx.recv().await {
        #[cfg(debug_assertions)]
        if let Some(rec) = opts.recorder.as_mut() {
            rec.append(r.at, &r.body);
        }
        let Some(head) = r.head else {
            tracing::warn!("hook payload not understood");
            continue;
        };
        let project = jump::project(head.cwd.as_deref().unwrap_or(""));
        tracing::info!(
            event = ?head.hook_event_name,
            session = ?head.session_id,
            reply_us = r.reply.as_micros() as u64,
            "hook"
        );
        // Walk only when the owner changed, PID or start time (ADR 0014).
        let found = r
            .lookup
            .filter(|l| l.process != s.sessions.process(&head.session_id))
            .and_then(|l| resolve(&s, &head.session_id, project, l));
        s.sessions.record(&head.session_id, project, found);
        #[cfg(debug_assertions)]
        if head.hook_event_name == "PermissionRequest" {
            let tool = head.tool_name.as_deref().unwrap_or("a tool");
            crate::toast::permission_request(&head.session_id, project, tool);
        }
        let mut stats = s.stats.lock().unwrap_or_else(|e| e.into_inner());
        stats.last_event_at = Some(SystemTime::now());
    }
}

/// §9.4 step 2 for a new or changed owner: walks up to the window and logs
/// the owner exe and precision.
fn resolve(
    s: &Shared,
    session: &str,
    project: &str,
    l: Lookup,
) -> Option<(ProcessId, Option<jump::Target>)> {
    let lookup_us = l.took.as_micros() as u64;
    let Some(process) = l.process else {
        tracing::warn!(session = ?session, lookup_us, "connection owner not found");
        return None;
    };
    let t = Instant::now();
    let target = s.platform.as_ref()?.find_window(process, project);
    tracing::info!(
        session = ?session,
        project = ?project,
        pid = process.pid,
        lookup_us,
        walk_us = t.elapsed().as_micros() as u64,
        owner = ?target.as_ref().map(|t| &t.owner),
        precision = ?target.as_ref().map(|t| t.precision),
        "session process"
    );
    Some((process, target))
}

fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

/// PHASE 0 ONLY: remove once fixtures are recorded (SPEC.md §14, §8.8).
///
/// Appends `{ "tMs", "hook", "payload" }` per request, with the RAW payload,
/// which contains agent free text. Hence: debug builds only, off unless
/// `KNOCK_RECORD_DIR` names an existing directory, and refused unless that
/// directory is under `%LOCALAPPDATA%` (not synced by OneDrive) and outside
/// any git repository (ADR 0008). Recordings must be sanitized before
/// anything goes into `fixtures/`.
#[cfg(debug_assertions)]
pub mod recorder {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::path::Path;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    pub struct Recorder {
        file: File,
        start: Instant,
    }

    pub(super) fn from_env() -> Option<Recorder> {
        let dir = std::env::var_os("KNOCK_RECORD_DIR")?;
        match open(Path::new(&dir)) {
            Ok(r) => {
                tracing::warn!("PHASE 0 recorder is on: raw payloads are being recorded");
                Some(r)
            }
            Err(e) => {
                tracing::error!(reason = %e, "PHASE 0 recorder refused to start");
                None
            }
        }
    }

    pub fn open(dir: &Path) -> io::Result<Recorder> {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::other("LOCALAPPDATA not set"))?;
        open_under(dir, Path::new(&local))
    }

    /// `open` with the allowed root passed in, so tests can use a temp root.
    pub(super) fn open_under(dir: &Path, root: &Path) -> io::Result<Recorder> {
        // Canonical on both sides, so short names, `..` and links can't slip out.
        let dir = dir.canonicalize()?;
        if !dir.starts_with(root.canonicalize()?) {
            return Err(io::Error::other("directory is not under %LOCALAPPDATA%"));
        }
        if dir.ancestors().any(|a| a.join(".git").exists()) {
            return Err(io::Error::other("directory is inside a git repository"));
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("knock-{secs}.jsonl")))?;
        Ok(Recorder {
            file,
            start: Instant::now(),
        })
    }

    impl Recorder {
        pub fn append(&mut self, at: Instant, body: &[u8]) {
            let Ok(payload) = serde_json::from_slice::<serde_json::Value>(body) else {
                return;
            };
            let line = serde_json::json!({
                "tMs": at.saturating_duration_since(self.start).as_millis() as u64,
                "hook": payload.get("hook_event_name"),
                "payload": payload,
            });
            let _ = writeln!(self.file, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};

    const TOKEN: &str = "test-token";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("knock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spawn_server() -> SocketAddr {
        spawn_server_with(Options::default(), None)
    }

    /// `log`: write this server's log lines to that folder.
    fn spawn_server_with(opts: Options, log: Option<PathBuf>) -> SocketAddr {
        let listener = bind(0).unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let _log = log.map(|d| tracing::subscriber::set_default(crate::log::FileLog::new(d)));
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(serve(listener, TOKEN.into(), opts))
        });
        addr
    }

    /// Raw HTTP/1.1 request; returns (status, body).
    fn request(
        addr: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: &[u8],
    ) -> (u16, String) {
        request_with(addr, method, path, token, "", body)
    }

    /// `request` with `extra` header lines, each ending in CRLF.
    fn request_with(
        addr: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        extra: &str,
        body: &[u8],
    ) -> (u16, String) {
        let mut s = TcpStream::connect(addr).unwrap();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n{extra}",
            body.len()
        );
        if let Some(t) = token {
            head += &format!("X-Knock-Token: {t}\r\n");
        }
        s.write_all(format!("{head}\r\n").as_bytes()).unwrap();
        s.write_all(body).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let status = out[9..12].parse().unwrap();
        let body = out.split_once("\r\n\r\n").unwrap().1.to_owned();
        (status, body)
    }

    const PAYLOAD: &[u8] = br#"{"session_id":"s1","hook_event_name":"Stop"}"#;

    #[test]
    fn hook_requires_the_right_token() {
        let addr = spawn_server();
        let hook = "/v1/hooks/claude-code";
        assert_eq!(request(addr, "POST", hook, None, PAYLOAD).0, 401);
        assert_eq!(
            request(addr, "POST", hook, Some("wrong-tokn"), PAYLOAD).0,
            401
        );
        assert_eq!(
            request(addr, "POST", hook, Some("test-token-longer"), PAYLOAD).0,
            401
        );
        assert_eq!(
            request(addr, "POST", hook, Some(TOKEN), PAYLOAD),
            (200, "{}".into())
        );
    }

    #[test]
    fn health_requires_token_and_counts_sessions() {
        let addr = spawn_server();
        assert_eq!(request(addr, "GET", "/v1/health", None, b"").0, 401);
        assert_eq!(request(addr, "GET", "/v1/health", Some("nope"), b"").0, 401);
        request(addr, "POST", "/v1/hooks/claude-code", Some(TOKEN), PAYLOAD);
        let health = wait_for_sessions(addr, 1);
        assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
        assert!(health["uptimeS"].is_u64());
        assert!(health["lastEventAt"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    // Release builds have no recorder field, so the update is a no-op there.
    #[allow(clippy::needless_update)]
    fn event_is_kept_when_the_client_gives_up_on_a_slow_reply() {
        let addr = spawn_server_with(
            Options {
                delay: Duration::from_secs(5),
                ..Options::default()
            },
            None,
        );
        let mut s = TcpStream::connect(addr).unwrap();
        write!(
            s,
            "POST /v1/hooks/claude-code HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Knock-Token: {TOKEN}\r\nContent-Length: {}\r\n\r\n",
            PAYLOAD.len()
        )
        .unwrap();
        s.write_all(PAYLOAD).unwrap();
        // Like Claude Code's hook timeout: hang up before the reply.
        std::thread::sleep(Duration::from_millis(100));
        drop(s);
        wait_for_sessions(addr, 1);
    }

    /// Records every connection-owner lookup and window walk.
    #[derive(Default)]
    struct FakePlatform {
        lookups: Mutex<Vec<(u16, u16)>>,
        /// What every lookup returns when set; otherwise the "PID" is the
        /// client port.
        owner: Mutex<Option<ProcessId>>,
        /// The PID of every `find_window` call.
        walks: Mutex<Vec<u32>>,
    }

    impl Platform for FakePlatform {
        fn primary_display(&self) -> io::Result<crate::platform::Display> {
            unimplemented!()
        }
        fn place_strip(&self, _: crate::strip::Rect) -> io::Result<()> {
            unimplemented!()
        }
        fn cursor_pos(&self) -> Option<(i32, i32)> {
            None
        }
        fn set_click_through(&self, _: bool) {}
        fn connection_owner(&self, client: u16, server: u16) -> Option<ProcessId> {
            self.lookups.lock().unwrap().push((client, server));
            Some(self.owner.lock().unwrap().unwrap_or(ProcessId {
                pid: client.into(),
                start: 1,
            }))
        }
        fn find_window(&self, p: ProcessId, _: &str) -> Option<jump::Target> {
            self.walks.lock().unwrap().push(p.pid);
            Some(jump::Target {
                window: 1,
                owner: "WindowsTerminal.exe".into(),
                precision: jump::Precision::AppWindow,
            })
        }
        fn focus_window(&self, _: isize) -> bool {
            false
        }
    }

    #[test]
    fn process_is_looked_up_once_per_session_with_the_peer_port() {
        let fake = Arc::new(FakePlatform::default());
        let sessions = Arc::new(Sessions::default());
        let opts = Options {
            platform: Some(fake.clone()),
            sessions: Arc::clone(&sessions),
            ..Options::default()
        };
        let addr = spawn_server_with(opts, None);
        let hook = "/v1/hooks/claude-code";
        let body = |id: &str| {
            format!(
                r#"{{"session_id":"{id}","hook_event_name":"Stop","cwd":"C:\\work\\repo-{id}","prompt":"secret"}}"#
            )
        };
        for id in ["a", "a", "b", "a"] {
            assert_eq!(
                request(addr, "POST", hook, Some(TOKEN), body(id).as_bytes()).0,
                200
            );
        }
        wait_for_sessions(addr, 2);
        // Wait for the worker to finish the last event.
        for _ in 0..100 {
            if sessions.views().first().is_some_and(|v| v.id == "a") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let calls = fake.lookups.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "one lookup per session: {calls:?}");
        for (client, server) in &calls {
            assert_eq!(*server, addr.port());
            assert!(*client != 0 && client != server);
        }
        let views = sessions.views();
        let json = serde_json::to_string(&views).unwrap();
        assert_eq!(
            json,
            r#"[{"id":"a","project":"repo-a","precision":"app_window"},{"id":"b","project":"repo-b","precision":"app_window"}]"#
        );
        // The first request's client port is the recorded "process".
        let (p, _, owner) = sessions.jump_info("a").unwrap();
        assert_eq!(p.pid, u32::from(calls[0].0));
        assert_eq!(owner.as_deref(), Some("WindowsTerminal.exe"));
    }

    #[test]
    fn simulated_requests_skip_the_process_lookup() {
        let fake = Arc::new(FakePlatform::default());
        let sessions = Arc::new(Sessions::default());
        let opts = Options {
            platform: Some(fake.clone()),
            sessions: Arc::clone(&sessions),
            ..Options::default()
        };
        let addr = spawn_server_with(opts, None);
        let hook = "/v1/hooks/claude-code";
        let body = |id: &str| format!(r#"{{"session_id":"{id}","hook_event_name":"Stop"}}"#);
        for _ in 0..2 {
            let sim = "X-Knock-Sim: 1\r\n";
            let r = request_with(addr, "POST", hook, Some(TOKEN), sim, body("sim").as_bytes());
            assert_eq!(r, (200, "{}".into()));
        }
        // Any other value is not a sim request: looked up as usual.
        let not_sim = "X-Knock-Sim: 0\r\n";
        request_with(
            addr,
            "POST",
            hook,
            Some(TOKEN),
            not_sim,
            body("real").as_bytes(),
        );
        wait_for_sessions(addr, 2);
        for _ in 0..100 {
            if sessions.process("real").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(sessions.process("real").is_some());
        assert_eq!(
            fake.lookups.lock().unwrap().len(),
            1,
            "only the real session"
        );
        assert!(sessions.process("sim").is_none());
        assert!(sessions.jump_info("sim").is_none());
        let view = sessions.views().into_iter().find(|v| v.id == "sim");
        assert_eq!(view.unwrap().precision, jump::Precision::None);
    }

    #[test]
    fn owner_is_rechecked_on_every_prompt_and_rewalked_only_when_it_changed() {
        let fake = Arc::new(FakePlatform::default());
        let sessions = Arc::new(Sessions::default());
        let opts = Options {
            platform: Some(fake.clone()),
            sessions: Arc::clone(&sessions),
            ..Options::default()
        };
        let addr = spawn_server_with(opts, None);
        let send = |event: &str, extra: &str| {
            let body = format!(r#"{{"session_id":"a","hook_event_name":"{event}"}}"#);
            let r = request_with(
                addr,
                "POST",
                "/v1/hooks/claude-code",
                Some(TOKEN),
                extra,
                body.as_bytes(),
            );
            assert_eq!(r, (200, "{}".into()));
        };
        // The worker runs in arrival order: once a sim marker for another
        // session is counted, every earlier event has been processed.
        let mut markers = 1;
        let mut settle = || {
            markers += 1;
            let body = format!(r#"{{"session_id":"m{markers}","hook_event_name":"Stop"}}"#);
            let sim = "X-Knock-Sim: 1
";
            request_with(
                addr,
                "POST",
                "/v1/hooks/claude-code",
                Some(TOKEN),
                sim,
                body.as_bytes(),
            );
            wait_for_sessions(addr, markers);
        };
        let lookups = || fake.lookups.lock().unwrap().len();
        let walks = || fake.walks.lock().unwrap().clone();
        let p1 = ProcessId { pid: 100, start: 1 };
        let p2 = ProcessId { pid: 200, start: 2 };

        *fake.owner.lock().unwrap() = Some(p1);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((lookups(), walks()), (1, vec![100]));
        // Not a prompt, process known: no lookup.
        send("PreToolUse", "");
        settle();
        assert_eq!(lookups(), 1);
        // Same owner on the next prompt: looked up, not re-walked.
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((lookups(), walks()), (2, vec![100]));
        // Same PID, new start time (a reused PID): a different owner.
        *fake.owner.lock().unwrap() = Some(ProcessId { pid: 100, start: 9 });
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((lookups(), walks()), (3, vec![100, 100]));
        // Same session id from a different owner: replaced.
        *fake.owner.lock().unwrap() = Some(p2);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((lookups(), walks()), (4, vec![100, 100, 200]));
        let (p, _, owner) = sessions.jump_info("a").unwrap();
        assert_eq!(p, p2);
        assert_eq!(owner.as_deref(), Some("WindowsTerminal.exe"));
        // Sim prompts skip all of it.
        send(
            "UserPromptSubmit",
            "X-Knock-Sim: 1
",
        );
        settle();
        assert_eq!((lookups(), walks().len()), (4, 3));
        assert_eq!(sessions.process("a"), Some(p2));
    }

    /// The worker processes after the reply; poll health briefly.
    fn wait_for_sessions(addr: SocketAddr, n: u64) -> serde_json::Value {
        let mut health = serde_json::Value::Null;
        for _ in 0..100 {
            let (status, body) = request(addr, "GET", "/v1/health", Some(TOKEN), b"");
            assert_eq!(status, 200);
            health = serde_json::from_str(&body).unwrap();
            if health["sessionCount"] == n {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(health["sessionCount"], n);
        health
    }

    /// A valid hook payload for `session`, padded with spaces to `len` bytes.
    fn padded(session: &str, len: usize) -> Vec<u8> {
        let mut b =
            format!(r#"{{"session_id":"{session}","hook_event_name":"Stop"}}"#).into_bytes();
        b.resize(len, b' ');
        b
    }

    #[test]
    fn body_over_16_mb_gets_200_and_is_dropped_with_a_warning() {
        assert_eq!(BODY_LIMIT, 16 * 1024 * 1024);
        let dir = temp_dir("body-limit-log");
        let addr = spawn_server_with(Options::default(), Some(dir.clone()));
        let hook = "/v1/hooks/claude-code";
        let ok = (200, "{}".to_owned());
        let at_limit = padded("at-limit", BODY_LIMIT);
        assert_eq!(request(addr, "POST", hook, Some(TOKEN), &at_limit), ok);
        wait_for_sessions(addr, 1);
        let over = padded("over-limit", BODY_LIMIT + 1);
        assert_eq!(request(addr, "POST", hook, Some(TOKEN), &over), ok);
        // One worker, in order: once "after" counts, "over-limit" would have.
        request(addr, "POST", hook, Some(TOKEN), &padded("after", 64));
        wait_for_sessions(addr, 2);
        let mut log = String::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            log += &std::fs::read_to_string(e.unwrap().path()).unwrap();
        }
        assert_eq!(log.matches("WARN").count(), 1, "{log}");
        assert!(log.contains("hook dropped: body over 16 MB"));
        assert!(!log.contains("over-limit"), "the body must not be logged");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Streams a `len`-byte hook body in 64 KB writes on one thread while
    /// reading the reply on this one, like a client still uploading when the
    /// server answers. Returns (upload result, reply).
    fn stream_hook(addr: SocketAddr, session: &str, len: usize) -> (io::Result<()>, String) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let mut w = s.try_clone().unwrap();
        let first = padded(session, 64 * 1024);
        let writer = std::thread::spawn(move || {
            write!(
                w,
                "POST /v1/hooks/claude-code HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nX-Knock-Token: {TOKEN}\r\nContent-Length: {len}\r\n\r\n"
            )?;
            w.write_all(&first)?;
            let spaces = [b' '; 64 * 1024];
            let mut left = len - first.len();
            while left > 0 {
                let n = left.min(spaces.len());
                w.write_all(&spaces[..n])?;
                left -= n;
            }
            Ok(())
        });
        let mut out = Vec::new();
        let read = s.read_to_end(&mut out);
        let upload = writer.join().unwrap();
        let reply = match read {
            Ok(_) => String::from_utf8_lossy(&out).into_owned(),
            Err(e) => format!("read failed: {:?}", e.kind()),
        };
        (upload, reply)
    }

    #[test]
    fn body_far_over_the_limit_is_read_to_the_end_then_gets_200() {
        // A log here too: a callsite first hit on a thread with no subscriber
        // can be cached as off for the other tests' logs.
        let dir = temp_dir("far-over-log");
        let addr = spawn_server_with(Options::default(), Some(dir.clone()));
        // Under the limit, streamed the same way, is still queued.
        let (upload, reply) = stream_hook(addr, "under", 1024 * 1024);
        assert!(upload.is_ok() && reply.ends_with("\r\n\r\n{}"), "{reply}");
        wait_for_sessions(addr, 1);
        for i in 0..3 {
            for mult in [2, 4] {
                let (upload, reply) = stream_hook(addr, "far-over", mult * BODY_LIMIT);
                assert!(upload.is_ok(), "{mult}x run {i}: upload {upload:?}");
                assert!(
                    reply.starts_with("HTTP/1.1 200"),
                    "{mult}x run {i}: {reply}"
                );
                assert!(reply.ends_with("\r\n\r\n{}"), "{mult}x run {i}: {reply}");
            }
        }
        let mut log = String::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            log += &std::fs::read_to_string(e.unwrap().path()).unwrap();
        }
        assert_eq!(log.matches("hook dropped").count(), 6, "{log}");
        assert_eq!(log.matches("WARN").count(), 6, "{log}");
        assert!(!log.contains("far-over"), "the body must not be logged");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generated_config_is_in_range_with_a_43_char_token() {
        let mut ports = HashSet::new();
        for _ in 0..2000 {
            let cfg = ServerConfig::generate().unwrap();
            assert!(PORTS.contains(&cfg.port));
            ports.insert(cfg.port);
            assert_eq!(cfg.token.len(), 43);
            assert!(
                cfg.token
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            );
        }
        assert!(ports.len() > 1000, "ports should be spread out");
        assert_ne!(
            ServerConfig::generate().unwrap().token,
            ServerConfig::generate().unwrap().token
        );
    }

    #[test]
    fn base64url_matches_rfc4648_vectors() {
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn server_json_round_trips_and_port_stays_fixed() {
        let dir = temp_dir("server-json");
        let (first, listener) = listen(&dir).unwrap();
        assert_eq!(listener.local_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
        drop(listener);
        let saved: ServerConfig =
            serde_json::from_slice(&std::fs::read(dir.join("server.json")).unwrap()).unwrap();
        assert_eq!(saved, first);
        let (second, _l) = listen(&dir).unwrap();
        assert_eq!(second, first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_path_and_method_needs_the_token() {
        let addr = spawn_server();
        for (method, path) in [
            ("GET", "/nope"),
            ("POST", "/v1/hooks/other"),
            ("GET", "/v1/hooks/claude-code"),
            ("POST", "/v1/health"),
        ] {
            for token in [None, Some("wrong")] {
                let status = request(addr, method, path, token, b"").0;
                assert_eq!(status, 401, "{method} {path} {token:?}");
            }
        }
        // With the token, routing answers as before.
        assert_eq!(request(addr, "GET", "/nope", Some(TOKEN), b"").0, 404);
        let hook = "/v1/hooks/claude-code";
        assert_eq!(request(addr, "GET", hook, Some(TOKEN), b"").0, 405);
    }

    #[test]
    fn server_json_with_a_bad_token_or_port_is_rejected() {
        const GOOD: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJ-_01234";
        let cfg = |port: u16, token: &str| {
            let token = token.into();
            serde_json::to_vec(&ServerConfig { port, token }).unwrap()
        };
        assert!(parse_config(&cfg(50000, GOOD)).is_ok());
        let long = format!("{GOOD}A");
        let plus = GOOD.replace('-', "+");
        let padded = format!("{}=", &GOOD[..42]);
        for token in [
            "",
            &GOOD[..42],
            &long,
            &plus,
            &padded,
            &GOOD.replace('a', "é"),
        ] {
            let e = parse_config(&cfg(50000, token)).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{token:?}");
        }
        for port in [0, 80, 49151] {
            assert!(parse_config(&cfg(port, GOOD)).is_err());
        }
        assert!(parse_config(b"{}").is_err());
        // And `listen` refuses to start on one.
        let dir = temp_dir("bad-server-json");
        std::fs::write(dir.join("server.json"), cfg(50000, "")).unwrap();
        let e = listen(&dir).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn queued_timestamps_never_go_backwards() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let s = Arc::new(Shared {
            token: TOKEN.into(),
            port: 0,
            platform: None,
            sessions: Arc::default(),
            started: Instant::now(),
            delay: Duration::ZERO,
            stats: Mutex::default(),
            tx: Mutex::new(tx),
        });
        // Every request "started" at the same moment; queue order follows
        // when each one is queued, across racing threads.
        let start = Instant::now();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let s = Arc::clone(&s);
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        enqueue(&s, start, Bytes::new(), None, None);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let (mut last, mut n) = (start, 0);
        while let Ok(r) = rx.try_recv() {
            assert!(r.at >= last, "timestamp went backwards in queue order");
            assert_eq!(r.reply, r.at - start);
            last = r.at;
            n += 1;
        }
        assert_eq!(n, 8 * 500);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn recorder_needs_localappdata_and_no_git_repo() {
        let root = temp_dir("recorder-root");
        let outside = temp_dir("recorder-outside");
        let inside = root.join("rec");
        let repo = root.join("repo");
        std::fs::create_dir(&inside).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir(repo.join("sub")).unwrap();

        let err = |d: &Path| recorder::open_under(d, &root).err().map(|e| e.to_string());
        assert!(err(&outside).unwrap().contains("LOCALAPPDATA"));
        // `..` can't climb out of the root.
        let climb = inside.join("..").join("..");
        assert!(err(&climb).unwrap().contains("LOCALAPPDATA"));
        assert!(err(&repo.join("sub")).unwrap().contains("git"));
        assert!(err(&inside).is_none());

        // Against the real %LOCALAPPDATA%: this repo is refused, a fresh
        // folder directly under it is accepted.
        assert!(recorder::open(Path::new(env!("CARGO_MANIFEST_DIR"))).is_err());
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
            .join(format!("knock-test-recorder-{}", std::process::id()));
        std::fs::create_dir_all(&local).unwrap();
        let ok = recorder::open(&local);
        assert!(ok.is_ok());
        drop(ok);
        for d in [&local, &root, &outside] {
            std::fs::remove_dir_all(d).unwrap();
        }
    }
}
