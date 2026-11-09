//! Local server (SPEC.md §8.1, §11). Binds 127.0.0.1 only, requires the token
//! on every request, answers hooks with `200 {}` at once and queues them, in
//! arrival order, for the pipeline's owner thread to parse and process. The
//! connection's owning process is looked up before the reply, while the
//! client's end of the connection still exists (§7.2 step 2, §9.4).

use crate::jump::ProcessId;
use crate::pipeline::{self, Msg, Shared};
use crate::platform::{Platform, fill_random};
use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::ops::RangeInclusive;
use std::path::Path;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
/// fixed. A port that can't be bound later is an error; Repair ([`repair`])
/// picks a new one (ADR 0023).
pub fn listen(dir: &Path) -> io::Result<(ServerConfig, TcpListener)> {
    match std::fs::read(dir.join("server.json")) {
        Ok(bytes) => {
            let cfg = parse_config(&bytes)?;
            let listener = bind(cfg.port)?;
            Ok((cfg, listener))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let (cfg, listener) = pick(ServerConfig::generate()?.token, None)?;
            save(dir, &cfg)?;
            Ok((cfg, listener))
        }
        Err(e) => Err(e),
    }
}

/// Repair (§8.1, ADR 0023), as one step: binds a new free port in
/// 49152-65535, never the saved one, with the saved token kept (a new token
/// only if `server.json` is missing or invalid, since then there is none to
/// keep); `start`s the server there; and only then saves `server.json`,
/// atomically. If binding or `start` fails, `server.json` is as it was. If
/// saving fails, `start`'s result is dropped (for a server, its stop
/// sender, so it stops) and `server.json` is as it was.
pub fn repair<T>(
    dir: &Path,
    start: impl FnOnce(&ServerConfig, TcpListener) -> io::Result<T>,
) -> io::Result<(ServerConfig, T)> {
    let old = std::fs::read(dir.join("server.json"))
        .ok()
        .and_then(|b| parse_config(&b).ok());
    let token = match &old {
        Some(c) => c.token.clone(),
        None => ServerConfig::generate()?.token,
    };
    let (cfg, listener) = pick(token, old.map(|c| c.port))?;
    let started = start(&cfg, listener)?;
    save(dir, &cfg)?;
    Ok((cfg, started))
}

/// Binds a random port in the range other than `avoid`, for `token`. Gives
/// up after 50 ports that can't be bound.
fn pick(token: String, avoid: Option<u16>) -> io::Result<(ServerConfig, TcpListener)> {
    for _ in 0..50 {
        let port = ServerConfig::generate()?.port;
        if Some(port) == avoid {
            continue;
        }
        if let Ok(listener) = bind(port) {
            return Ok((ServerConfig { port, token }, listener));
        }
    }
    Err(no_free_port())
}

fn no_free_port() -> io::Error {
    io::Error::new(io::ErrorKind::AddrInUse, "no free port found")
}

/// Repair's `{ ok: false, message }` text for its error (§6 plain language).
/// Only "no free port" ([`pick`]'s AddrInUse) asks for a restart, since
/// Windows may free reserved ports then; any other step that failed changed
/// nothing and can simply be retried (ADR 0023).
pub fn repair_failed(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::AddrInUse => {
            "Knock couldn't find a free connection to listen on. Please restart your computer, then try Repair again."
        }
        _ => "Knock couldn't set up a new connection, so nothing changed. Please try Repair again.",
    }
}

fn save(dir: &Path, cfg: &ServerConfig) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(cfg)?;
    crate::settings::write_atomic(&dir.join("server.json"), &bytes)
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

/// Serves on `listener` (from [`listen`] or [`repair`]) on Tauri's async
/// runtime until `stop` fires or is dropped. Every step that can fail runs
/// here, before the task starts, so a listener that can't be used is an
/// `Err` (ADR 0023). Once started it can't fail: axum 0.8's `serve` retries
/// accept errors itself and returns only after `stop`.
pub fn spawn(
    cfg: ServerConfig,
    listener: TcpListener,
    platform: Arc<dyn Platform>,
    tx: Sender<Msg>,
    published: Shared,
    stop: tokio::sync::oneshot::Receiver<()>,
) -> io::Result<()> {
    let rt = tauri::async_runtime::handle();
    let (port, listener) = {
        // Inside the runtime, so the listener can join its reactor.
        let _in = rt.inner().enter();
        ready(listener)?
    };
    tracing::info!(port, "server listening on 127.0.0.1");
    let opts = Options {
        platform: Some(platform),
        stop: Some(stop),
        ..Options::from_env()
    };
    rt.spawn(async move {
        serve(port, listener, cfg.token, opts, tx, published).await;
        tracing::info!(port, "server stopped for a new port");
    });
    Ok(())
}

/// The fallible setup of serving: nonblocking, its port, and onto the
/// current tokio runtime (call inside one).
fn ready(listener: TcpListener) -> io::Result<(u16, tokio::net::TcpListener)> {
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    // An unbound socket has no address on Windows, but port 0 on Linux; the
    // hooks could never reach either.
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "listener has no port",
        ));
    }
    Ok((port, tokio::net::TcpListener::from_std(listener)?))
}

/// The health check's "server listening" (FR-12): a real `GET /v1/health`
/// to 127.0.0.1:`port` with the token answers `200` within 1 s. Blocking.
pub fn probe(port: u16, token: &str) -> bool {
    use std::io::{Read, Write};
    let t = Duration::from_secs(1);
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, t) else {
        return false;
    };
    let _ = (s.set_read_timeout(Some(t)), s.set_write_timeout(Some(t)));
    let req = format!(
        "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\n{TOKEN_HEADER}: {token}\r\nConnection: close\r\n\r\n"
    );
    let mut head = [0u8; 12];
    s.write_all(req.as_bytes()).is_ok() && s.read_exact(&mut head).is_ok() && &head[9..12] == b"200"
}

/// Tests leave `platform` out, so no process lookups happen.
#[derive(Default)]
pub struct Options {
    /// For the connection-owner lookup (§9.4).
    pub platform: Option<Arc<dyn Platform>>,
    /// Debug builds: sleep this long before replying, to test a slow Knock (V-2).
    pub delay: Duration,
    /// The server stops when this fires or its sender is dropped (Repair).
    /// `None`: it runs until the process ends.
    pub stop: Option<tokio::sync::oneshot::Receiver<()>>,
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
                ..Self::default()
            }
        }
        #[cfg(not(debug_assertions))]
        Self::default()
    }
}

struct Server {
    token: String,
    /// The port clients connect to, for the connection-owner lookup.
    port: u16,
    platform: Option<Arc<dyn Platform>>,
    published: Shared,
    started: Instant,
    delay: Duration,
    /// Locked so the receive time is taken and the hook queued in one step:
    /// queue order then always matches time order. Holds the last time
    /// given out, so a clock step back can't reorder them either.
    tx: Mutex<(u64, Sender<Msg>)>,
}

/// Serves on `listener` (from [`ready`], on `port`) until `opts.stop`.
pub async fn serve(
    port: u16,
    listener: tokio::net::TcpListener,
    token: String,
    opts: Options,
    tx: Sender<Msg>,
    published: Shared,
) {
    let stop = opts.stop;
    let server = Arc::new(Server {
        token,
        port,
        platform: opts.platform,
        published,
        started: Instant::now(),
        delay: opts.delay,
        tx: Mutex::new((0, tx)),
    });
    let app = Router::new()
        .route("/v1/hooks/claude-code", post(hook))
        .route("/v1/health", get(health));
    #[cfg(debug_assertions)]
    let app = app.route("/v1/debug/snapshot", get(snapshot));
    let app = app
        // Token check before routing, so unknown paths and wrong methods get
        // 401 too (§11.1).
        .layer(from_fn_with_state(Arc::clone(&server), require_token))
        .with_state(server);
    // Peer addresses give the client port for the connection-owner lookup.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        match stop {
            Some(rx) => drop(rx.await),
            None => std::future::pending().await,
        }
    })
    .await
    // axum 0.8: "will never error"; it returns only after the signal.
    .ok();
}

fn authorized(token: &str, headers: &HeaderMap) -> bool {
    headers.get(TOKEN_HEADER).is_some_and(|v| {
        let (a, b) = (v.as_bytes(), token.as_bytes());
        // Constant-time compare.
        a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
    })
}

async fn require_token(State(s): State<Arc<Server>>, req: Request, next: Next) -> Response {
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
    State(s): State<Arc<Server>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let sim = req.headers().get(SIM_HEADER).is_some_and(|v| v == "1");
    // Queued before any further await: if the client gives up, the event is
    // still processed.
    match read_capped(req.into_body()).await {
        Ok(Some(body)) => {
            // Every real request is looked up (about 0.3 ms, ADR 0012); the
            // pipeline uses the result on a session's first request and on
            // prompts (§9.4, ADR 0014). A simulated session has no real
            // process: no lookup (§8.1).
            let owner = (s.platform.as_ref())
                .filter(|_| !sim)
                .and_then(|p| p.connection_owner(peer.port(), s.port));
            enqueue(&s, body, sim, owner);
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
async fn read_capped(mut body: Body) -> Result<Option<Vec<u8>>, axum::Error> {
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
    Ok((!over).then_some(buf))
}

/// Queues a hook with its receive time (§8.3 `ts`), which never decreases
/// in queue order.
fn enqueue(s: &Server, body: Vec<u8>, sim: bool, owner: Option<ProcessId>) {
    let mut tx = s.tx.lock().unwrap_or_else(|e| e.into_inner());
    let ts = pipeline::now_ms().max(tx.0);
    tx.0 = ts;
    let _ = tx.1.send(Msg::Hook {
        ts,
        body,
        sim,
        owner,
    });
}

async fn health(State(s): State<Arc<Server>>) -> Response {
    let p = pipeline::read(&s.published);
    let body = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptimeS": s.started.elapsed().as_secs(),
        "sessionCount": p.sessions.sessions.len(),
        "lastEventAt": p.last_event_at.map(knock_core::iso8601),
    });
    json(body.to_string())
}

/// Debug builds only (§8.1): `{ rev, sessions }`, as in `sessions-changed`.
#[cfg(debug_assertions)]
async fn snapshot(State(s): State<Arc<Server>>) -> Response {
    let p = pipeline::read(&s.published);
    json(serde_json::to_string(&p.sessions).unwrap_or_default())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::FakePlatform;
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    const TOKEN: &str = "test-token";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("knock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spawn_server() -> SocketAddr {
        spawn_server_with(Options::default(), None).0
    }

    /// A server with a pipeline owner behind it, each on its own thread, the
    /// owner's files in a fresh temp folder. `log`: write this server's log
    /// lines to that folder.
    fn spawn_server_with(opts: Options, log: Option<PathBuf>) -> (SocketAddr, Shared) {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = temp_dir(&format!("server-{n}"));
        let published = Shared::default();
        let now = pipeline::now_ms();
        let core = pipeline::Core::new(dir, opts.platform.clone(), published.clone(), now, 720);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || pipeline::run(core, rx, |_| {}));
        let listener = bind(0).unwrap();
        let addr = listener.local_addr().unwrap();
        let p = published.clone();
        std::thread::spawn(move || {
            let _log = log.map(|d| tracing::subscriber::set_default(crate::log::FileLog::new(d)));
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let (port, listener) = ready(listener).unwrap();
                    serve(port, listener, TOKEN.into(), opts, tx, p).await
                })
        });
        (addr, published)
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

    /// ADR 0023: a listener that can't be served on is an `Err` from
    /// `spawn` itself, so startup shows Repair and Repair says it failed.
    #[test]
    fn spawn_fails_at_once_for_a_listener_it_cannot_use() {
        // A TCP socket never bound: it has no port.
        let unbound = tokio::net::TcpSocket::new_v4().unwrap();
        #[cfg(windows)]
        let socket = std::os::windows::io::AsSocket::as_socket(&unbound);
        #[cfg(unix)]
        let socket = std::os::fd::AsFd::as_fd(&unbound);
        let listener = TcpListener::from(socket.try_clone_to_owned().unwrap());
        let cfg = ServerConfig {
            port: 49152,
            token: TOKEN.into(),
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let platform = Arc::new(FakePlatform::default());
        let r = spawn(cfg, listener, platform, tx, Shared::default(), stopped);
        assert!(r.is_err());
        // A usable one starts, and serves until stopped.
        let (cfg, listener) = (
            ServerConfig {
                port: 0,
                token: TOKEN.into(),
            },
            bind(0).unwrap(),
        );
        let port = listener.local_addr().unwrap().port();
        let (tx, _rx) = std::sync::mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let platform = Arc::new(FakePlatform::default());
        spawn(cfg, listener, platform, tx, Shared::default(), stopped).unwrap();
        assert!(probe(port, TOKEN));
        drop(stop);
        let t = Instant::now();
        while probe(port, TOKEN) {
            assert!(t.elapsed() < Duration::from_secs(5), "still serving");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn probe_needs_the_server_and_the_right_token() {
        let addr = spawn_server();
        assert!(probe(addr.port(), TOKEN));
        assert!(!probe(addr.port(), "wrong"));
        let free = bind(0).unwrap().local_addr().unwrap().port();
        assert!(!probe(free, TOKEN));
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
    fn event_is_kept_when_the_client_gives_up_on_a_slow_reply() {
        let opts = Options {
            delay: Duration::from_secs(5),
            ..Options::default()
        };
        let addr = spawn_server_with(opts, None).0;
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

    fn with_fake() -> (SocketAddr, Shared, Arc<FakePlatform>) {
        let fake = Arc::new(FakePlatform::default());
        let platform: Arc<dyn Platform> = fake.clone();
        let opts = Options {
            platform: Some(platform),
            ..Options::default()
        };
        let (addr, published) = spawn_server_with(opts, None);
        (addr, published, fake)
    }

    /// The owner handles hooks in arrival order: once a sim marker for a new
    /// session is counted, every earlier hook has been processed.
    fn mark(addr: SocketAddr, markers: &mut u64) {
        *markers += 1;
        let body = format!(r#"{{"session_id":"m{markers}","hook_event_name":"Stop"}}"#);
        let sim = "X-Knock-Sim: 1\r\n";
        request_with(addr, "POST", HOOK, Some(TOKEN), sim, body.as_bytes());
    }

    const HOOK: &str = "/v1/hooks/claude-code";

    #[test]
    fn real_requests_are_looked_up_with_the_peer_port_and_walked_once_per_session() {
        let (addr, published, fake) = with_fake();
        let body = |id: &str| {
            format!(
                r#"{{"session_id":"{id}","hook_event_name":"Stop","cwd":"C:\\work\\repo-{id}","prompt":"secret"}}"#
            )
        };
        for id in ["a", "a", "b", "a"] {
            let r = request(addr, "POST", HOOK, Some(TOKEN), body(id).as_bytes());
            assert_eq!(r, (200, "{}".into()));
        }
        let mut markers = 0;
        mark(addr, &mut markers);
        wait_for_sessions(addr, 3);
        let calls = fake.lookups.lock().unwrap().clone();
        assert_eq!(calls.len(), 4, "every real request: {calls:?}");
        for (client, server) in &calls {
            assert_eq!(*server, addr.port());
            assert!(*client != 0 && client != server);
        }
        assert_eq!(fake.walks.lock().unwrap().len(), 2, "one walk per session");
        let p = pipeline::read(&published);
        let views: Vec<_> = (p.sessions.sessions.iter())
            .map(|v| (v.id.as_str(), v.project.as_str(), v.precision, v.state))
            .collect();
        let app = crate::jump::Precision::AppWindow;
        let none = crate::jump::Precision::None;
        assert_eq!(
            views,
            [
                ("a", "repo-a", app, "ready"),
                ("b", "repo-b", app, "ready"),
                ("m1", "", none, "ready"),
            ]
        );
        // The first request's client port is the recorded "process".
        assert_eq!(p.jump["a"].0.pid, u32::from(calls[0].0));
        assert!(
            !serde_json::to_string(&p.sessions)
                .unwrap()
                .contains("secret")
        );
    }

    #[test]
    fn simulated_requests_skip_the_process_lookup() {
        let (addr, published, fake) = with_fake();
        let body = |id: &str| format!(r#"{{"session_id":"{id}","hook_event_name":"Stop"}}"#);
        for _ in 0..2 {
            let sim = "X-Knock-Sim: 1\r\n";
            let r = request_with(addr, "POST", HOOK, Some(TOKEN), sim, body("sim").as_bytes());
            assert_eq!(r, (200, "{}".into()));
        }
        // Any other value is not a sim request: looked up as usual.
        let not_sim = "X-Knock-Sim: 0\r\n";
        request_with(
            addr,
            "POST",
            HOOK,
            Some(TOKEN),
            not_sim,
            body("real").as_bytes(),
        );
        wait_for_sessions(addr, 2);
        assert_eq!(fake.lookups.lock().unwrap().len(), 1, "only the real one");
        let p = pipeline::read(&published);
        assert!(p.jump.contains_key("real") && !p.jump.contains_key("sim"));
        let sim = p.sessions.sessions.iter().find(|v| v.id == "sim").unwrap();
        assert_eq!(sim.precision, crate::jump::Precision::None);
        assert!(sim.simulated);
    }

    #[test]
    fn owner_is_rechecked_on_every_prompt_and_rewalked_only_when_it_changed() {
        let (addr, published, fake) = with_fake();
        let send = |event: &str, extra: &str| {
            let body = format!(r#"{{"session_id":"a","hook_event_name":"{event}"}}"#);
            let r = request_with(addr, "POST", HOOK, Some(TOKEN), extra, body.as_bytes());
            assert_eq!(r, (200, "{}".into()));
        };
        let mut markers = 0;
        let mut settle = || {
            mark(addr, &mut markers);
            wait_for_sessions(addr, 1 + markers);
        };
        let walks = || fake.walks.lock().unwrap().clone();
        let owner = || pipeline::read(&published).jump["a"].0;
        let p1 = ProcessId { pid: 100, start: 1 };
        let p2 = ProcessId { pid: 200, start: 2 };

        *fake.owner.lock().unwrap() = Some(p1);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!(walks(), [100]);
        // Not a prompt: a different owner is not taken.
        *fake.owner.lock().unwrap() = Some(p2);
        send("PreToolUse", "");
        settle();
        assert_eq!((walks(), owner()), (vec![100], p1));
        // Same owner on the next prompt: not re-walked.
        *fake.owner.lock().unwrap() = Some(p1);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!(walks(), [100]);
        // Same PID, new start time (a reused PID): a different owner.
        let reused = ProcessId { pid: 100, start: 9 };
        *fake.owner.lock().unwrap() = Some(reused);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((walks(), owner()), (vec![100, 100], reused));
        // Same session id from a different owner: replaced.
        *fake.owner.lock().unwrap() = Some(p2);
        send("UserPromptSubmit", "");
        settle();
        assert_eq!((walks(), owner()), (vec![100, 100, 200], p2));
        let lookups = fake.lookups.lock().unwrap().len();
        // Sim prompts skip all of it.
        send("UserPromptSubmit", "X-Knock-Sim: 1\r\n");
        settle();
        assert_eq!(fake.lookups.lock().unwrap().len(), lookups);
        assert_eq!((walks().len(), owner()), (3, p2));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_snapshot_needs_the_token_and_returns_session_views() {
        let addr = spawn_server();
        assert_eq!(request(addr, "GET", "/v1/debug/snapshot", None, b"").0, 401);
        let (status, body) = request(addr, "GET", "/v1/debug/snapshot", Some(TOKEN), b"");
        assert_eq!((status, body.as_str()), (200, r#"{"rev":0,"sessions":[]}"#));
        let ask = br#"{"session_id":"s1","hook_event_name":"PermissionRequest","tool_name":"Bash","cwd":"C:\\w\\demo","tool_input":{"command":"secret"}}"#;
        request_with(addr, "POST", HOOK, Some(TOKEN), "X-Knock-Sim: 1\r\n", ask);
        wait_for_sessions(addr, 1);
        let (_, body) = request(addr, "GET", "/v1/debug/snapshot", Some(TOKEN), b"");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["rev"], 1);
        let s = &v["sessions"][0];
        assert_eq!(
            (s["id"].as_str(), s["project"].as_str()),
            (Some("s1"), Some("demo"))
        );
        assert_eq!(
            (s["tier"].as_str(), s["state"].as_str()),
            (Some("needs_you"), Some("needs_input"))
        );
        assert_eq!(
            s["needs"],
            serde_json::json!({ "kind": "permission", "label": "Bash", "seen": false })
        );
        assert_eq!(
            (s["precision"].as_str(), s["simulated"].as_bool()),
            (Some("none"), Some(true))
        );
        assert!(!body.contains("secret"));
    }

    /// The owner processes after the reply; poll health briefly.
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
        let addr = spawn_server_with(Options::default(), Some(dir.clone())).0;
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
        let addr = spawn_server_with(Options::default(), Some(dir.clone())).0;
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

    /// A port in the §8.1 range that this test now holds on 127.0.0.1.
    fn hold_a_port() -> TcpListener {
        (0..200)
            .find_map(|_| bind(ServerConfig::generate().unwrap().port).ok())
            .unwrap()
    }

    #[test]
    fn repair_picks_a_new_free_port_and_keeps_the_token() {
        let dir = temp_dir("repair");
        let (first, listener) = listen(&dir).unwrap();
        drop(listener);
        // Another program takes Knock's port (a real bind conflict).
        let blocker = hold_a_port();
        let taken = blocker.local_addr().unwrap().port();
        let cfg = ServerConfig {
            port: taken,
            token: first.token.clone(),
        };
        std::fs::write(dir.join("server.json"), serde_json::to_vec(&cfg).unwrap()).unwrap();
        let e = listen(&dir).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AddrInUse, "{e:?}");
        // Repair: another port in range, same token, saved, bound, no temp file.
        let (fixed, listener) = repair(&dir, |_, l| Ok(l)).unwrap();
        assert_ne!(fixed.port, taken);
        assert!(PORTS.contains(&fixed.port));
        assert_eq!(fixed.token, first.token);
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr, SocketAddr::from((Ipv4Addr::LOCALHOST, fixed.port)));
        let saved = parse_config(&std::fs::read(dir.join("server.json")).unwrap()).unwrap();
        assert_eq!(saved, fixed);
        assert!(!dir.join("server.json.tmp").exists());
        // The new port is fixed from now on.
        drop(listener);
        assert_eq!(listen(&dir).unwrap().0, fixed);
        drop(blocker);
        // Never the saved port, even when it is free again.
        for _ in 0..20 {
            std::fs::write(dir.join("server.json"), serde_json::to_vec(&fixed).unwrap()).unwrap();
            let (next, _l) = repair(&dir, |_, l| Ok(l)).unwrap();
            assert_ne!(next.port, fixed.port);
            assert_eq!(next.token, first.token);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// ADR 0023: Repair saves `server.json` only once the new server has
    /// started; if starting or saving fails, the file stays as it was.
    #[test]
    fn repair_saves_server_json_only_after_the_server_started() {
        let dir = temp_dir("repair-order");
        let file = dir.join("server.json");
        let (first, listener) = listen(&dir).unwrap();
        drop(listener);
        let before = std::fs::read(&file).unwrap();
        // Starting fails: nothing saved, and the error comes back.
        let refused = || Err::<(), _>(io::Error::other("refused"));
        let e = repair(&dir, |_, _| refused()).unwrap_err();
        assert_eq!(e.to_string(), "refused");
        assert_eq!(std::fs::read(&file).unwrap(), before);
        // While the server starts, the old file is still there.
        let (cfg, seen) = repair(&dir, |cfg, l| {
            assert_eq!(l.local_addr()?.port(), cfg.port);
            std::fs::read(&file)
        })
        .unwrap();
        assert_eq!(seen, before, "saved before the server started");
        assert_eq!(parse_config(&std::fs::read(&file).unwrap()).unwrap(), cfg);
        assert_eq!(cfg.token, first.token);
        // Saving fails (a folder is in the way): the started server's stop
        // sender is dropped, so it stops, and the error comes back.
        let dir = temp_dir("repair-save-fails");
        std::fs::create_dir(dir.join("server.json")).unwrap();
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let e = repair(&dir, |_, _| Ok(stop)).unwrap_err();
        let retry =
            "Knock couldn't set up a new connection, so nothing changed. Please try Repair again.";
        assert_eq!(repair_failed(&e), retry);
        // Only "no free port" asks for a restart.
        let restart = "Knock couldn't find a free connection to listen on. Please restart your computer, then try Repair again.";
        assert_eq!(repair_failed(&no_free_port()), restart);
        assert_eq!(repair_failed(&io::Error::other("refused")), retry);
        assert_eq!(
            stopped.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(dir.join("server.json").is_dir());
        assert!(!dir.join("server.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn repair_without_a_valid_server_json_makes_a_new_token() {
        let dir = temp_dir("repair-invalid");
        std::fs::write(dir.join("server.json"), b"{ not json").unwrap();
        assert_eq!(listen(&dir).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let (cfg, _l) = repair(&dir, |_, l| Ok(l)).unwrap();
        let saved = parse_config(&std::fs::read(dir.join("server.json")).unwrap()).unwrap();
        assert_eq!(saved, cfg);
        let dir2 = temp_dir("repair-none");
        let (cfg2, _l2) = repair(&dir2, |_, l| Ok(l)).unwrap();
        assert_ne!(cfg2.token, cfg.token);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir2).unwrap();
    }

    #[test]
    fn a_server_stops_when_its_stop_sender_is_dropped() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let opts = Options {
            stop: Some(rx),
            ..Options::default()
        };
        let addr = spawn_server_with(opts, None).0;
        assert!(probe(addr.port(), TOKEN));
        drop(tx);
        let stopped = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            !probe(addr.port(), TOKEN)
        });
        assert!(stopped, "still answering");
        // The port is free again.
        drop(bind(addr.port()).unwrap());
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
        let (tx, rx) = std::sync::mpsc::channel();
        let s = Arc::new(Server {
            token: TOKEN.into(),
            port: 0,
            platform: None,
            published: Shared::default(),
            started: Instant::now(),
            delay: Duration::ZERO,
            // As if the clock had just stepped back an hour.
            tx: Mutex::new((pipeline::now_ms() + 3_600_000, tx)),
        });
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let s = Arc::clone(&s);
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        enqueue(&s, Vec::new(), false, None);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let (mut last, mut n) = (0, 0);
        while let Ok(Msg::Hook { ts, .. }) = rx.try_recv() {
            assert!(ts >= last, "timestamp went backwards in queue order");
            last = ts;
            n += 1;
        }
        assert_eq!(n, 8 * 500);
    }
}
