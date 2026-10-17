//! Local server (SPEC.md §8.1, §11). Binds 127.0.0.1 only, requires the token
//! on every request, answers hooks with `200 {}` at once and processes them
//! afterwards on a single worker, in arrival order.

use crate::log::utc;
use crate::platform::fill_random;
use axum::Router;
use axum::body::{Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, TcpListener};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc;

pub const BODY_LIMIT: usize = 256 * 1024;
pub const PORTS: RangeInclusive<u16> = 49152..=65535;
const TOKEN_HEADER: &str = "x-knock-token";

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
            let cfg: ServerConfig = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            if !PORTS.contains(&cfg.port) {
                return Err(io::Error::other("server.json port out of range"));
            }
            let listener = bind(cfg.port)?;
            Ok((cfg, listener))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            for _ in 0..10 {
                let cfg = ServerConfig::generate()?;
                if let Ok(listener) = bind(cfg.port) {
                    std::fs::create_dir_all(dir)?;
                    let tmp = dir.join("server.json.tmp");
                    std::fs::write(&tmp, serde_json::to_vec_pretty(&cfg)?)?;
                    std::fs::rename(&tmp, &path)?;
                    return Ok((cfg, listener));
                }
            }
            Err(io::Error::other("no free port found"))
        }
        Err(e) => Err(e),
    }
}

fn bind(port: u16) -> io::Result<TcpListener> {
    // Loopback only, never 0.0.0.0 (§8.1, §11.1).
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}

/// Starts the server on Tauri's async runtime. Called once by the app.
pub fn start() -> io::Result<()> {
    let base = std::env::var_os("APPDATA").ok_or_else(|| io::Error::other("APPDATA not set"))?;
    let (cfg, listener) = listen(&PathBuf::from(base).join("Knock"))?;
    listener.set_nonblocking(true)?;
    tracing::info!(port = cfg.port, "server listening on 127.0.0.1");
    let opts = Options::from_env();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = serve(listener, cfg.token, opts).await {
            tracing::error!(kind = ?e.kind(), "server stopped");
        }
    });
    Ok(())
}

/// Debug-build knobs for Phase 0 tests.
#[derive(Default)]
pub struct Options {
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
            }
        }
        #[cfg(not(debug_assertions))]
        Self::default()
    }
}

struct Shared {
    token: String,
    started: Instant,
    delay: Duration,
    stats: Mutex<Stats>,
    tx: mpsc::UnboundedSender<Received>,
}

#[derive(Default)]
struct Stats {
    sessions: HashSet<String>,
    last_event_at: Option<SystemTime>,
}

struct Received {
    /// Arrival time; read only by the Phase 0 recorder.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    at: Instant,
    reply: Duration,
    body: Bytes,
}

pub async fn serve(listener: TcpListener, token: String, opts: Options) -> io::Result<()> {
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let (tx, rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared {
        token,
        started: Instant::now(),
        delay: opts.delay,
        stats: Mutex::default(),
        tx,
    });
    tokio::spawn(worker(Arc::clone(&shared), rx, opts));
    let app = Router::new()
        .route("/v1/hooks/claude-code", post(hook))
        .route("/v1/health", get(health))
        .with_state(shared);
    axum::serve(listener, app).await
}

fn authorized(token: &str, headers: &HeaderMap) -> bool {
    headers.get(TOKEN_HEADER).is_some_and(|v| {
        let (a, b) = (v.as_bytes(), token.as_bytes());
        // Constant-time compare.
        a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
    })
}

fn json(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

async fn hook(State(s): State<Arc<Shared>>, req: Request) -> Response {
    let at = Instant::now();
    if !authorized(&s.token, req.headers()) {
        // Never log the body or the header value.
        tracing::warn!("hook rejected: missing or wrong token");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(body) = to_bytes(req.into_body(), BODY_LIMIT).await else {
        tracing::warn!("hook rejected: body over 256 KB or unreadable");
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    // Queue before any further await: if the client gives up, the event is
    // still processed. `reply` is the time to here (§6: under 100 ms).
    let _ = s.tx.send(Received {
        at,
        reply: at.elapsed(),
        body,
    });
    if !s.delay.is_zero() {
        tokio::time::sleep(s.delay).await;
    }
    // Never any decision fields (§11.2).
    json("{}".into())
}

async fn health(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if !authorized(&s.token, &headers) {
        tracing::warn!("health rejected: missing or wrong token");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let stats = s.stats.lock().unwrap_or_else(|e| e.into_inner());
    json(
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptimeS": s.started.elapsed().as_secs(),
            "sessionCount": stats.sessions.len(),
            "lastEventAt": stats.last_event_at.map(utc),
        })
        .to_string(),
    )
}

/// The only fields read from a payload in this spike. Everything else,
/// including all free text, is skipped by the parser and never kept.
#[derive(Deserialize)]
struct Head {
    session_id: String,
    hook_event_name: String,
}

#[cfg_attr(not(debug_assertions), allow(unused_mut, unused_variables))]
async fn worker(s: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<Received>, mut opts: Options) {
    while let Some(r) = rx.recv().await {
        #[cfg(debug_assertions)]
        if let Some(rec) = opts.recorder.as_mut() {
            rec.append(r.at, &r.body);
        }
        let Ok(head) = serde_json::from_slice::<Head>(&r.body) else {
            tracing::warn!("hook payload not understood");
            continue;
        };
        tracing::info!(
            event = ?head.hook_event_name,
            session = ?head.session_id,
            reply_us = r.reply.as_micros() as u64,
            "hook"
        );
        let mut stats = s.stats.lock().unwrap_or_else(|e| e.into_inner());
        stats.sessions.insert(head.session_id);
        stats.last_event_at = Some(SystemTime::now());
    }
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
/// `KNOCK_RECORD_DIR` names an existing directory, and refused if that
/// directory is inside a git repository. Recordings must be sanitized before
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
        let dir = dir.canonicalize()?;
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
        spawn_server_with(Options::default())
    }

    fn spawn_server_with(opts: Options) -> SocketAddr {
        let listener = bind(0).unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
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
        let mut s = TcpStream::connect(addr).unwrap();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
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
        let health = wait_for_one_session(addr);
        assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
        assert!(health["uptimeS"].is_u64());
        assert!(health["lastEventAt"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    // Release builds have no recorder field, so the update is a no-op there.
    #[allow(clippy::needless_update)]
    fn event_is_kept_when_the_client_gives_up_on_a_slow_reply() {
        let addr = spawn_server_with(Options {
            delay: Duration::from_secs(5),
            ..Options::default()
        });
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
        wait_for_one_session(addr);
    }

    /// The worker processes after the reply; poll health briefly.
    fn wait_for_one_session(addr: SocketAddr) -> serde_json::Value {
        let mut health = serde_json::Value::Null;
        for _ in 0..100 {
            let (status, body) = request(addr, "GET", "/v1/health", Some(TOKEN), b"");
            assert_eq!(status, 200);
            health = serde_json::from_str(&body).unwrap();
            if health["sessionCount"] == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(health["sessionCount"], 1);
        health
    }

    #[test]
    fn body_limit_is_256_kb() {
        let addr = spawn_server();
        let hook = "/v1/hooks/claude-code";
        let at_limit = vec![b' '; BODY_LIMIT];
        assert_eq!(request(addr, "POST", hook, Some(TOKEN), &at_limit).0, 200);
        let over = vec![b' '; BODY_LIMIT + 1];
        assert_eq!(request(addr, "POST", hook, Some(TOKEN), &over).0, 413);
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

    #[cfg(debug_assertions)]
    #[test]
    fn recorder_refuses_a_directory_inside_the_repo() {
        let in_repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(recorder::open(in_repo).is_err());
        let outside = temp_dir("recorder");
        assert!(recorder::open(&outside).is_ok());
        std::fs::remove_dir_all(&outside).unwrap();
    }
}
