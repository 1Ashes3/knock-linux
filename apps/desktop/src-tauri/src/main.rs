//! Knock desktop shell (SPEC.md §7.1, §7.2): the strip window (§9.1-9.3), the
//! local server (§8.1), the pipeline and its effect runner, the tray (§9.7)
//! and the §8.5 commands and events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod helper;
mod hooks;
mod jump;
mod log;
mod packs;
mod pipeline;
mod platform;
mod server;
mod settings;
mod strip;
mod tray;

use pipeline::{Msg, Out, SessionsChanged, Shared};
use platform::{Monitor, NativePlatform, Platform, StripLayer};
use serde_json::{Value, json};
use settings::{Placement, Settings};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use strip::{Region, Strip};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

/// About 30 cursor reads a second (§9.3).
const POLL: Duration = Duration::from_millis(33);
/// FR-15: the strip is re-placed this often even if no change was heard.
const FALLBACK: Duration = Duration::from_secs(5);
/// How often the strip thread re-checks whether the strip should be up.
const RISE_CHECK: Duration = Duration::from_millis(500);
/// After a change was heard, placed again this much later, in case the
/// shell was still settling (the taskbar sliding, a display waking).
const RECHECK: Duration = Duration::from_millis(500);

/// `setup_status.serverError` texts (ADR 0023).
const CANT_START: &str = "Knock couldn't start listening for Claude Code. Another program may be using its connection. Choose Repair to give Knock a new one.";

/// The local server: why it isn't running, and how to stop it.
#[derive(Default)]
struct Listening {
    /// Plain text while the server isn't running (ADR 0023).
    error: Option<&'static str>,
    /// Dropping it stops the running server (Repair replaces it).
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

/// What the commands share. Reads come from [`Shared`]; changes go to the
/// pipeline's owner as messages, so no command waits on a lock it holds.
struct Knock {
    tx: Sender<Msg>,
    shared: Shared,
    platform: Arc<dyn Platform>,
    strip: Arc<Mutex<Strip>>,
    /// Set to have the strip placed again at once (FR-15).
    moved: Arc<AtomicBool>,
    /// Claude Code hook setup (§8.2).
    hooks: hooks::Setup,
    server: Arc<Mutex<Listening>>,
    /// One Repair at a time.
    repairing: Mutex<()>,
    /// `%APPDATA%\Knock`.
    dir: PathBuf,
    /// No Knock `settings.json` existed when the app started.
    first_run: bool,
    /// A `.knockpack` opened with Knock (a double-click), until Settings
    /// asks for it (ADR 0038).
    offered: Mutex<Option<PathBuf>>,
}

/// The `.knockpack` among a launch's arguments, if any (a double-click in a
/// file manager runs `knock-desktop <file>`).
fn knockpack_arg(args: &[String]) -> Option<PathBuf> {
    args.iter().skip(1).map(PathBuf::from).find(|p| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("knockpack"))
            && p.is_file()
    })
}

/// Remembers a `.knockpack` to offer and opens Settings, which asks for it
/// (`take_offered_pack`), and tells an open Settings window.
fn offer_from_launch(app: &AppHandle, path: PathBuf) {
    if let Some(k) = app.try_state::<Knock>() {
        *lock(&k.offered) = Some(path);
    }
    open_settings(app, "pack=1");
    let _ = app.emit_to("settings", "pack-offered", ());
}

/// ADR 0037: which layer the strip is on, from the `placement` setting and
/// whether a knock nobody has seen is waiting. The platform is told only
/// when the layer changes.
struct StripView {
    platform: Arc<dyn Platform>,
    /// (placement, knocking, the layer last set)
    state: Mutex<(Placement, bool, Option<StripLayer>)>,
}

impl StripView {
    fn new(platform: Arc<dyn Platform>, placement: Placement) -> Self {
        Self {
            platform,
            state: Mutex::new((placement, false, None)),
        }
    }

    fn update(&self, change: impl FnOnce(&mut Placement, &mut bool)) {
        let layer = {
            let mut s = lock(&self.state);
            let (placement, knocking, last) = &mut *s;
            change(placement, knocking);
            let layer = StripLayer::of(*placement, *knocking);
            if *last == Some(layer) {
                return;
            }
            *last = Some(layer);
            layer
        };
        // Outside the lock: the platform may wait for the main thread.
        self.platform.set_strip_layer(layer);
    }
}

/// How long a finished session keeps the strip up if nobody clicks it.
const READY_RISE_MS: u64 = 2 * 60_000;

/// The strip rises for a session (ADR 0037): a knock nobody has seen yet
/// (ADR 0016), or a session that finished under [`READY_RISE_MS`] ago and
/// hasn't been reviewed (a click turns Ready into Idle). The overlay's
/// `rises` in placement.ts is the same rule.
fn rises(s: &pipeline::SessionView, now: u64) -> bool {
    let knock = s.needs.as_ref().is_some_and(|n| !n.seen);
    let done = s.state == "ready" && now < s.since_ms.saturating_add(READY_RISE_MS);
    knock || done
}

fn knocking(sessions: &SessionsChanged, now: u64) -> bool {
    sessions.sessions.iter().any(|s| rises(s, now))
}

/// Locks, ignoring poisoning: every value here stays usable.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// §8.5: the current `{ rev, sessions }`, as in `sessions-changed`. The
/// overlay calls it when it loads. An `away-summary` sent before that is
/// not sent again (§8.5 has no command for it); it is only logged.
#[tauri::command]
fn get_sessions(k: tauri::State<'_, Knock>) -> SessionsChanged {
    let p = pipeline::read(&k.shared);
    if p.away_pending {
        tracing::info!("overlay loaded with an away summary pending; not sent again");
    }
    p.sessions.clone()
}

/// §8.5, §9.5, §9.6: focuses the session's window, then acknowledges it
/// (ADR 0016). Sync, so it runs on the main thread, the one that received
/// the click.
#[tauri::command]
fn jump_to_session(id: String, k: tauri::State<'_, Knock>) {
    jump(&k, id, "strip");
}

/// A session with no known process or window only acknowledges (§9.5).
fn jump(k: &Knock, id: String, via: &str) {
    let info = pipeline::read(&k.shared).jump.get(&id).cloned();
    match info {
        None => tracing::info!(session = ?id, via, "jump: no process"),
        Some((process, project)) => {
            let t = Instant::now();
            // Looked up again on every jump: the window may have closed or moved.
            match k.platform.find_window(process, &project) {
                None => tracing::info!(session = ?id, via, "jump: no window"),
                Some(target) => {
                    let front = k.platform.focus_window(target.window);
                    tracing::info!(
                        session = ?id,
                        via,
                        precision = ?target.precision,
                        front,
                        took_us = t.elapsed().as_micros() as u64,
                        "jump"
                    );
                }
            }
        }
    }
    let _ = k.tx.send(Msg::Ack(id));
}

/// §8.5: mark Ready as reviewed (`user.ack`).
#[tauri::command]
fn acknowledge(id: String, k: tauri::State<'_, Knock>) {
    let _ = k.tx.send(Msg::Ack(id));
}

/// §8.5: dismiss Failed or ended (`user.dismiss`).
#[tauri::command]
fn dismiss(id: String, k: tauri::State<'_, Knock>) {
    let _ = k.tx.send(Msg::Dismiss(id));
}

/// §8.5, FR-9, ADR 0034: the away card folded away (a click or 60 s).
#[tauri::command]
fn dismiss_away_summary(k: tauri::State<'_, Knock>) {
    let _ = k.tx.send(Msg::DismissAway);
}

/// §8.5, ADR 0003: replaces the strip's hit regions; empty means fully
/// click-through.
#[tauri::command]
fn set_hit_regions(regions: Vec<Region>, k: tauri::State<'_, Knock>) {
    k.platform.set_hit_regions(&regions);
    lock(&k.strip).regions = regions;
}

/// §8.5, §9.2: the size the UI needs, in logical px. The strip keeps its
/// bottom-right corner and is at most 6 slots wide; width 0 (no sessions)
/// leaves a zero-width strip. The placement thread moves it within one
/// poll tick, on the chosen display, never activating it (§9.1).
#[tauri::command]
fn request_strip_size(width: f64, height: f64, k: tauri::State<'_, Knock>) -> Result<(), String> {
    strip::clamp(width, height, 1.0, 1.0).ok_or("width and height must be numbers of 0 or more")?;
    lock(&k.strip).want = Some((width, height));
    k.moved.store(true, Ordering::Relaxed);
    Ok(())
}

/// §9.2, FR-15: puts the strip where [`strip::layout`] says for the current
/// settings and displays, if it isn't there already. Holds no lock while
/// moving the window, so a command on the main thread can't deadlock it.
fn reposition(p: &dyn Platform, strip: &Mutex<Strip>, shared: &Shared) {
    let (monitor, slot) = {
        let s = &pipeline::read(shared).settings;
        (s.monitor.clone(), s.size.height())
    };
    let want = lock(strip).want;
    let Some(l) = strip::layout(&monitor, &p.displays(), p.taskbar(), want, slot) else {
        tracing::warn!("no display found; strip not placed");
        return;
    };
    let was = std::mem::replace(&mut lock(strip).fell_back, l.fell_back);
    if l.fell_back && !was {
        tracing::warn!("chosen monitor not connected; strip on the primary");
    }
    if p.strip_rect() != Some(l.rect) {
        if let Err(e) = p.place_strip(l.rect) {
            tracing::warn!(kind = ?e.kind(), "strip not placed");
            return;
        }
        tracing::info!(rect = ?l.rect, scale = l.scale, "strip placed");
    }
    let mut s = lock(strip);
    (s.rect, s.scale) = (l.rect, l.scale);
}

/// §8.6.
/// ADR 0038: every pack, classic first; invalid ones say why.
#[tauri::command]
fn list_packs(packs: tauri::State<'_, Arc<packs::Packs>>) -> Vec<packs::PackSummary> {
    packs.list()
}

/// ADR 0038: what the overlay needs to draw an installed pack; `null` for
/// classic or a pack that isn't installed and valid.
#[tauri::command]
fn get_pack(id: String, packs: tauri::State<'_, Arc<packs::Packs>>) -> Option<packs::PackData> {
    packs.get(&id)
}

/// ADR 0038: reads the packs folder again, re-casts sessions if the active
/// pack changed, and tells every window (`packs-changed`).
#[tauri::command]
fn reload_packs(
    app: AppHandle,
    packs: tauri::State<'_, Arc<packs::Packs>>,
    k: tauri::State<'_, Knock>,
) -> Vec<packs::PackSummary> {
    packs.reload();
    let _ = k.tx.send(Msg::PacksChanged);
    let _ = app.emit("packs-changed", ());
    packs.list()
}

/// ADR 0038: shows the packs folder (created if missing) in the file
/// manager, for artists. Users add packs with "Add a pack…" instead.
#[tauri::command]
fn open_packs_folder(packs: tauri::State<'_, Arc<packs::Packs>>) -> Result<(), String> {
    let dir = packs.dir().ok_or("Knock has no packs folder here.")?;
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Couldn't create the packs folder ({}).", e.kind()))?;
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Couldn't open the packs folder.".into())
}

/// ADR 0038: checks the `.knockpack` file or pack folder at `path` (a drop
/// on Settings) and holds it for `install_pack`.
#[tauri::command]
fn offer_pack_path(path: String, packs: tauri::State<'_, Arc<packs::Packs>>) -> packs::Offer {
    packs.offer_path(std::path::Path::new(&path))
}

/// ADR 0038: the same for a `.knockpack` the file picker read; the body is
/// the file's bytes.
#[tauri::command]
fn offer_pack_bytes(
    request: tauri::ipc::Request<'_>,
    packs: tauri::State<'_, Arc<packs::Packs>>,
) -> Result<packs::Offer, String> {
    match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => Ok(packs.offer_bytes(bytes)),
        _ => Err("Knock didn't get the file.".into()),
    }
}

/// ADR 0038: the `.knockpack` Knock was opened with, offered; `null` if none.
#[tauri::command]
fn take_offered_pack(
    k: tauri::State<'_, Knock>,
    packs: tauri::State<'_, Arc<packs::Packs>>,
) -> Option<packs::Offer> {
    let path = lock(&k.offered).take()?;
    Some(packs.offer_path(&path))
}

/// ADR 0038: installs the offered pack, then every window reads the packs
/// again (`packs-changed`).
#[tauri::command]
fn install_pack(
    token: String,
    replace: bool,
    app: AppHandle,
    packs: tauri::State<'_, Arc<packs::Packs>>,
    k: tauri::State<'_, Knock>,
) -> Result<Vec<packs::PackSummary>, String> {
    packs.install(&token, replace)?;
    let _ = k.tx.send(Msg::PacksChanged);
    let _ = app.emit("packs-changed", ());
    Ok(packs.list())
}

/// ADR 0038: removes an installed pack, or hides a built-in one (it is part
/// of the app); if it was the chosen one, sessions go back to Classic.
#[tauri::command]
fn remove_pack(
    id: String,
    app: AppHandle,
    packs: tauri::State<'_, Arc<packs::Packs>>,
    k: tauri::State<'_, Knock>,
) -> Result<Vec<packs::PackSummary>, String> {
    let settings = pipeline::read(&k.shared).settings.clone();
    let mut patch = serde_json::Map::new();
    if packs.is_builtin(&id) {
        // Part of the app: hidden instead, and "Bring back" undoes it.
        let mut hidden = settings.hidden_packs.clone();
        if !hidden.contains(&id) {
            hidden.push(id.clone());
        }
        patch.insert("hiddenPacks".into(), json!(hidden));
    } else {
        packs.remove(&id)?;
    }
    if settings.pack == id {
        patch.insert("pack".into(), json!(packs::CLASSIC));
    }
    if !patch.is_empty() {
        let patch = Value::Object(patch);
        let _ = k.tx.send(Msg::Settings { patch, reply: None });
    }
    let _ = k.tx.send(Msg::PacksChanged);
    let _ = app.emit("packs-changed", ());
    Ok(packs.list())
}

/// `knockpack://<pack>/<path>` (ADR 0038): a valid pack's images, from
/// memory. Anything else is a 404. The images may not load anything
/// themselves (default-src 'none'), and are readable cross-origin so the
/// overlay can draw them into its canvas.
fn knockpack(
    app: &AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let found = packs::request_path(request.uri().path())
        .and_then(|(pack, path)| app.try_state::<Arc<packs::Packs>>()?.image(&pack, &path));
    let builder = tauri::http::Response::builder()
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'",
        )
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store");
    let response = match found {
        Some((bytes, media)) => builder
            .status(200)
            .header("Content-Type", media)
            .body(bytes),
        None => builder.status(404).body(Vec::new()),
    };
    response.unwrap_or_default()
}

#[tauri::command]
fn get_settings(k: tauri::State<'_, Knock>) -> Settings {
    pipeline::read(&k.shared).settings.clone()
}

/// §8.6: merges `patch`, validates, saves, applies at once and emits
/// `settings-changed`. Async, so the main thread never waits on the owner.
/// `launchAtSignIn: true/false` also adds or removes the sign-in entry
/// (§9.7), in step with the save (`pipeline::Core`).
#[tauri::command]
async fn update_settings(
    patch: serde_json::Value,
    k: tauri::State<'_, Knock>,
) -> Result<Settings, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let stopping = || "Knock is shutting down.".to_string();
    let msg = Msg::Settings {
        patch,
        reply: Some(tx),
    };
    k.tx.send(msg).map_err(|_| stopping())?;
    rx.await.map_err(|_| stopping())?
}

/// `{ ok: false, message }` for a setup problem, else `ok(t)`.
fn reply<T>(r: Result<T, hooks::Problem>, ok: impl FnOnce(T) -> Value) -> Value {
    match r {
        Ok(t) => ok(t),
        Err(p) => {
            tracing::info!(problem = ?p, "hook setup stopped");
            json!({ "ok": false, "message": p.message() })
        }
    }
}

/// The real server check (FR-12), off the async workers: it blocks up to 1 s.
async fn server_ok(k: &Knock) -> bool {
    if lock(&k.server).error.is_some() {
        return false;
    }
    let Some((port, token)) = k.hooks.server() else {
        return false;
    };
    let probe = move || server::probe(port, &token);
    tauri::async_runtime::spawn_blocking(probe)
        .await
        .unwrap_or(false)
}

/// §8.5, §10.4. `hooksInstalled` only for the current port and token.
#[tauri::command]
async fn setup_status(k: tauri::State<'_, Knock>) -> Result<Value, String> {
    Ok(json!({
        "firstRun": k.first_run,
        "hooksInstalled": k.hooks.installed().unwrap_or(false),
        "serverOk": server_ok(&k).await,
        "serverError": lock(&k.server).error,
        "monitorCount": k.platform.monitors().len(),
    }))
}

/// §8.1, ADR 0023: a new free port, the same token, the server started
/// there, then `server.json` saved, then the old server (if any) stopped.
/// If any step fails, the old server and `server.json` stay as they were.
/// Claude Code's hooks still name the old port, so the UI runs Connect next.
#[tauri::command]
async fn repair_server(app: AppHandle, k: tauri::State<'_, Knock>) -> Result<Value, String> {
    let _one = lock(&k.repairing);
    let r = server::repair(&k.dir, |cfg, listener| serve(&k, cfg, listener));
    Ok(match r {
        Ok((cfg, stop)) => {
            adopt(&app, &k, cfg, stop);
            tracing::info!("server repaired");
            json!({ "ok": true })
        }
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "server not repaired");
            json!({ "ok": false, "message": server::repair_failed(&e) })
        }
    })
}

/// Starts a server on `listener` with `cfg`'s token, beside any running
/// one. Dropping the returned sender stops it.
fn serve(
    k: &Knock,
    cfg: &server::ServerConfig,
    listener: TcpListener,
) -> std::io::Result<tokio::sync::oneshot::Sender<()>> {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let (platform, tx, shared) = (Arc::clone(&k.platform), k.tx.clone(), k.shared.clone());
    // Once started, the server can't stop on its own (`server::spawn`).
    server::spawn(cfg.clone(), listener, platform, tx, shared, stopped)?;
    Ok(stop)
}

/// Makes the server [`serve`] started the current one: stops the old one,
/// tells hook setup the port and token, and clears the server error.
fn adopt(
    app: &AppHandle,
    k: &Knock,
    cfg: server::ServerConfig,
    stop: tokio::sync::oneshot::Sender<()>,
) {
    k.hooks.set_server(Some((cfg.port, cfg.token)));
    lock(&k.server).stop = Some(stop);
    server_down(app, k, None);
}

/// Records why the server isn't running (`None`: it is), and shows it in
/// the tray tooltip (ADR 0023).
fn server_down(app: &AppHandle, k: &Knock, error: Option<&'static str>) {
    lock(&k.server).error = error;
    post_tooltip(app, &k.server, &k.shared);
}

/// Sets the tray tooltip from the server error and Needs-you count as they
/// are when the main thread runs it, so whichever post runs last shows the
/// current state.
fn post_tooltip(app: &AppHandle, server: &Arc<Mutex<Listening>>, shared: &Shared) {
    let (h, server, shared) = (app.clone(), Arc::clone(server), shared.clone());
    let _ = app.run_on_main_thread(move || {
        let down = lock(&server).error.is_some();
        let n = (pipeline::read(&shared).sessions.sessions.iter())
            .filter(|v| v.tier == knock_core::Tier::NeedsYou.as_str())
            .count();
        if let Some(icon) = h.tray_by_id("knock") {
            let _ = icon.set_tooltip(Some(tray::tooltip(n, down)));
        }
    });
}

/// §8.5: every display, primary first; `id` is the §8.6 `monitor` value.
#[tauri::command]
async fn setup_list_monitors(k: tauri::State<'_, Knock>) -> Result<Vec<Monitor>, String> {
    Ok(k.platform.monitors())
}

/// §8.2 step 2: the whole file before and after, token masked. Writes nothing.
#[tauri::command]
async fn setup_preview_hooks(k: tauri::State<'_, Knock>) -> Result<Value, String> {
    Ok(reply(
        k.hooks.preview(),
        |p| json!({ "ok": true, "path": p.path, "before": p.before, "after": p.after }),
    ))
}

/// §8.2 steps 3-7: after the user confirmed the preview.
#[tauri::command]
async fn setup_apply_hooks(k: tauri::State<'_, Knock>) -> Result<Value, String> {
    let r = k.hooks.apply(pipeline::now_ms());
    Ok(reply(r, |backup| json!({ "ok": true, "backup": backup })))
}

/// §8.2 removal.
#[tauri::command]
async fn setup_remove_hooks(k: tauri::State<'_, Knock>) -> Result<Value, String> {
    let r = k.hooks.remove(pipeline::now_ms());
    Ok(reply(r, |backup| json!({ "ok": true, "backup": backup })))
}

#[derive(serde::Serialize)]
struct Check {
    id: &'static str,
    label: &'static str,
    ok: bool,
    detail: String,
}

/// FR-12: real checks, in plain language, with no path but the file name
/// and no token.
#[tauri::command]
async fn run_health_check(k: tauri::State<'_, Knock>) -> Result<Vec<Check>, String> {
    let hooks = match k.hooks.status() {
        Ok(hooks::Hooks::Installed) => {
            (true, "Knock is in your Claude Code settings (settings.json).".into())
        }
        Ok(hooks::Hooks::Stale) => (
            false,
            "Claude Code still points at Knock's old connection. Run setup again.".into(),
        ),
        Ok(hooks::Hooks::Missing) => (
            false,
            "Knock isn't in your Claude Code settings (settings.json), or its entry is out of date. Run setup to connect it."
                .into(),
        ),
        Err(p) => (false, p.message().to_string()),
    };
    let server = match server_ok(&k).await {
        true => (
            true,
            "Knock is listening for Claude Code, on this computer only.".into(),
        ),
        false => (
            false,
            "Knock isn't listening for Claude Code. Choose Repair to fix it.".into(),
        ),
    };
    let last = pipeline::read(&k.shared).last_event_at;
    let event = match last {
        Some(t) => (
            true,
            format!(
                "Knock last heard from Claude Code {}.",
                ago(pipeline::now_ms().saturating_sub(t))
            ),
        ),
        None => (
            false,
            "Knock hasn't heard from Claude Code since it started. Send a message in a Claude Code window, then check again."
                .into(),
        ),
    };
    let check = |id, label, (ok, detail): (bool, String)| Check {
        id,
        label,
        ok,
        detail,
    };
    Ok(vec![
        check("hooks", "Connected to Claude Code", hooks),
        check("server", "Listening for Claude Code", server),
        check("lastEvent", "Last event from Claude Code", event),
    ])
}

/// "just now", "5 minutes ago", "1 hour ago", "2 days ago".
fn ago(ms: u64) -> String {
    let s = ms / 1000;
    let (n, unit) = match s {
        0..60 => return "just now".into(),
        60..3600 => (s / 60, "minute"),
        3600..86_400 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// §10.4 step 5: a simulated session knocks, with its toast, then leaves.
#[tauri::command]
fn play_test_knock(k: tauri::State<'_, Knock>) {
    let _ = k.tx.send(Msg::TestKnock);
}

/// Opens the settings window, or brings it back if it is open. `query`:
/// `setup=1` starts the setup flow (§10.4), `repair=1` the Repair screen
/// (ADR 0023), empty the settings. A normal, focusable window, unlike the
/// strip. From an event handler, call it off the main thread: creating a
/// window there deadlocks on Windows.
fn open_settings(app: &AppHandle, query: &str) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = (w.unminimize(), w.show(), w.set_focus());
        return;
    }
    let url = match query {
        "" => "/settings/index.html".to_string(),
        q => format!("/settings/index.html?{q}"),
    };
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(url.into()))
        .title("Knock Settings")
        .inner_size(640.0, 720.0)
        .min_inner_size(420.0, 480.0)
        .center()
        .build();
    if built.is_err() {
        tracing::warn!("settings window not opened");
    }
}

/// The effect runner's output side, on the owner thread. Tray and menu
/// setters wait for the main thread, so they are posted to it rather than
/// called here: a command on the main thread may be waiting on the owner.
fn apply(
    app: &AppHandle,
    tray: &tray::Tray,
    out: Out,
    server: &Arc<Mutex<Listening>>,
    shared: &Shared,
    moved: &AtomicBool,
    view: &StripView,
) {
    match out {
        Out::Toast {
            session,
            title,
            body,
        } => {
            let toast = app.notification().builder().title(title).body(body);
            // The session id comes back on a click (ADR 0015).
            if toast.extra("session", session).show().is_err() {
                tracing::warn!("toast not shown");
            }
        }
        Out::Sessions(payload) => {
            let on = knocking(&payload, pipeline::now_ms());
            view.update(|_, k| *k = on);
            let _ = app.emit_to("strip", "sessions-changed", payload);
        }
        Out::Away(payload) => {
            let _ = app.emit_to("strip", "away-summary", payload);
        }
        Out::Settings(settings) => {
            // A new `monitor` or `size` moves the strip at once.
            moved.store(true, Ordering::Relaxed);
            view.update(|p, _| *p = settings.placement);
            let (item, on) = (tray.pause.clone(), settings.pause_notifications);
            let places = tray.placement.clone();
            let fades = tray.fade.clone();
            let (placement, fade) = (settings.placement, settings.card_fade_seconds);
            let _ = app.run_on_main_thread(move || {
                let _ = item.set_checked(on);
                for (p, item) in places {
                    let _ = item.set_checked(p == placement);
                }
                for (n, item) in fades {
                    let _ = item.set_checked(n == fade);
                }
            });
            let _ = app.emit(
                "settings-changed",
                serde_json::json!({ "settings": settings }),
            );
        }
        Out::Tray(n) => {
            let icon = tray.icon.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = icon.set_icon(tray::badge(n));
            });
            post_tooltip(app, server, shared);
        }
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Created hidden; shown by the platform layer only once its §9.1 styles
    // and position are applied (below). Never call anything that focuses it.
    let url = WebviewUrl::App("/overlay/index.html".into());
    let builder = WebviewWindowBuilder::new(app, "strip", url)
        .title("Knock")
        .visible(false)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .resizable(false)
        .shadow(false);
    #[cfg(windows)]
    let builder = builder.no_redirection_bitmap(true);
    let window = builder.build()?;
    let platform: Arc<dyn Platform> = Arc::new(NativePlatform::new(&window)?);

    let dir = settings::data_dir()?;
    // Holds the server token and Claude settings backups: owner only.
    if let Err(e) = settings::make_private_dir(&dir) {
        tracing::warn!(kind = ?e.kind(), "data folder not made private");
    }
    let first_run = !dir.join(settings::FILE).exists();
    let shared = Shared::default();
    let now = pipeline::now_ms();
    let mut core = pipeline::Core::new(
        dir.clone(),
        Some(Arc::clone(&platform)),
        shared.clone(),
        now,
        platform::local_minute(),
    );
    // Sprite packs (ADR 0038): read and checked once here, then on Reload.
    let packs = Arc::new(packs::Packs::new(knock_core::paths::packs_dir(
        &knock_core::paths::process_env,
    )));
    core.set_packs(Arc::clone(&packs));
    app.manage(Arc::clone(&packs));
    let (tx, rx) = std::sync::mpsc::channel();
    let tray = tray::build(app.handle(), tx.clone(), shared.clone())?;
    let listening = Arc::new(Mutex::new(Listening::default()));
    let moved = Arc::new(AtomicBool::new(false));
    let handle = app.handle().clone();
    let placement = pipeline::read(&shared).settings.placement;
    let view = Arc::new(StripView::new(Arc::clone(&platform), placement));
    let (l, sh, m, v) = (
        Arc::clone(&listening),
        shared.clone(),
        Arc::clone(&moved),
        Arc::clone(&view),
    );
    std::thread::Builder::new()
        .name("knock-pipeline".into())
        .spawn(move || {
            pipeline::run(core, rx, |out| apply(&handle, &tray, out, &l, &sh, &m, &v))
        })?;

    // A click on the on-screen toast jumps to its session (ADR 0015). The
    // handler runs on the plugin's thread; the jump runs on the main one.
    let handle = app.handle().clone();
    app.notification().on_action(move |a| {
        let session = (a.notification())
            .and_then(|n| n.extra().get("session"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let (Some(id), "tap") = (session, a.action_id()) else {
            return;
        };
        let h = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(k) = h.try_state::<Knock>() {
                jump(&k, id, "toast");
            }
        });
    })?;

    let spec = knock_core::Adapter::hooks(&knock_core::ClaudeCode { users: vec![] });
    let claude = hooks::claude_settings();
    tracing::info!(
        path_set = claude.is_some(),
        test_file = cfg!(debug_assertions) && claude.is_some(),
        "claude settings file"
    );
    let mut hooks = hooks::Setup::new(spec, claude, dir.join("backups"), None);
    hooks.guard = hooks::guarded();

    let strip = Arc::new(Mutex::new(Strip {
        rect: strip::Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        scale: 1.0,
        regions: Vec::new(),
        want: None,
        fell_back: false,
    }));
    let k = Knock {
        tx,
        shared,
        platform,
        strip,
        moved,
        hooks,
        server: listening,
        repairing: Mutex::new(()),
        dir,
        first_run,
        offered: Mutex::new(knockpack_arg(&std::env::args().collect::<Vec<_>>())),
    };

    // If the port can't be bound, Knock keeps running and offers Repair
    // (§8.1, ADR 0023).
    let started = server::listen(&k.dir).and_then(|(cfg, listener)| {
        let stop = serve(&k, &cfg, listener)?;
        adopt(app.handle(), &k, cfg, stop);
        Ok(())
    });
    if let Err(e) = &started {
        tracing::error!(kind = ?e.kind(), "server failed to start");
        server_down(app.handle(), &k, Some(CANT_START));
    }

    let (p, s, sh) = (&*k.platform, &*k.strip, &k.shared);
    tracing::info!(displays = ?p.displays(), taskbar = ?p.taskbar(), "displays");
    reposition(p, s, sh);
    // The layer before the first show, so a hidden strip never flashes.
    view.update(|_, _| {});
    p.show_strip();
    // Display, DPI, work-area and taskbar messages reach the strip's window
    // on this (main) thread.
    p.watch_displays(Arc::clone(&k.moved));

    let (p, s, sh, moved, v) = (
        Arc::clone(&k.platform),
        Arc::clone(&k.strip),
        k.shared.clone(),
        Arc::clone(&k.moved),
        Arc::clone(&view),
    );
    std::thread::Builder::new()
        .name("knock-strip".into())
        .spawn(move || {
            let (mut due, mut again) = (Instant::now() + FALLBACK, false);
            let mut rise_check = Instant::now();
            loop {
                std::thread::sleep(POLL);
                // A finished session stops raising the strip after a while
                // with no new event (ADR 0037); checked twice a second.
                if rise_check.elapsed() >= RISE_CHECK {
                    rise_check = Instant::now();
                    let on = knocking(&pipeline::read(&sh).sessions, pipeline::now_ms());
                    v.update(|_, k| *k = on);
                }
                // FR-15: within one tick of a change, again shortly after,
                // and every 5 s anyway.
                let now = Instant::now();
                if moved.swap(false, Ordering::Relaxed) {
                    (due, again) = (now, true);
                }
                if now >= due {
                    reposition(&*p, &s, &sh);
                    let wait = if std::mem::take(&mut again) {
                        RECHECK
                    } else {
                        FALLBACK
                    };
                    due = now + wait;
                }
                // Where the cursor can't be read (Wayland), the platform
                // applies the hit regions itself (ADR 0037).
                if cfg!(windows) {
                    let inside = p.cursor_pos().is_some_and(|c| {
                        let s = lock(&s);
                        strip::hit(&s.regions, s.rect, s.scale, c)
                    });
                    // Every tick, so styles tao rewrites are repaired within
                    // one tick (ADR 0001). It writes only when they differ.
                    p.set_click_through(!inside);
                }
            }
        })?;

    // §10.4: setup opens by itself on first run, or while Knock isn't
    // connected to Claude Code; Repair first if the server isn't running.
    let installed = k.hooks.installed().unwrap_or(false);
    tracing::info!(
        first_run,
        installed,
        server = started.is_ok(),
        "setup status"
    );
    let offered = lock(&k.offered).is_some();
    app.manage(k);
    match (started.is_ok(), first_run || !installed) {
        (false, _) => open_settings(app.handle(), "repair=1"),
        (true, true) => open_settings(app.handle(), "setup=1"),
        // Started by a double-click on a .knockpack.
        (true, false) if offered => open_settings(app.handle(), "pack=1"),
        (true, false) => {}
    }
    Ok(())
}

fn main() {
    // `knock-desktop hook <agent>`: another agent's hook command (ADR 0039).
    // Before anything else starts, and it never returns to the app.
    helper::run_if_asked();
    log::init();
    tauri::Builder::default()
        // First, so a second launch exits before anything else starts (§9.7).
        // It brings Settings to the front instead: an open setup or Repair
        // screen is focused, not replaced; the strip is never focused (§9.1).
        // The plugin lets this instance take the foreground from the second.
        // Its arguments and folder are not logged (§11.5).
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            tracing::info!("second launch: opening settings");
            // Off the main thread: creating a window in this callback (a
            // window procedure) deadlocks on Windows.
            let app = app.clone();
            let pack = knockpack_arg(&args);
            std::thread::spawn(move || match pack {
                Some(path) => offer_from_launch(&app, path),
                None => open_settings(&app, ""),
            });
        }))
        .plugin(tauri_plugin_notification::init())
        .register_uri_scheme_protocol("knockpack", |ctx, request| {
            knockpack(ctx.app_handle(), &request)
        })
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            jump_to_session,
            acknowledge,
            dismiss,
            dismiss_away_summary,
            set_hit_regions,
            request_strip_size,
            get_settings,
            update_settings,
            setup_status,
            setup_list_monitors,
            setup_preview_hooks,
            setup_apply_hooks,
            setup_remove_hooks,
            run_health_check,
            play_test_knock,
            repair_server,
            list_packs,
            get_pack,
            reload_packs,
            open_packs_folder,
            offer_pack_path,
            offer_pack_bytes,
            take_offered_pack,
            install_pack,
            remove_pack,
        ])
        .setup(setup)
        .run(tauri::generate_context!())
        .expect("error while running Knock");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(state: &'static str, since_ms: u64, seen: Option<bool>) -> pipeline::SessionView {
        pipeline::SessionView {
            id: "s".into(),
            agent: "claude-code",
            project: "p".into(),
            tier: "ready",
            state,
            since: String::new(),
            since_ms,
            activity: None,
            needs: seen.map(|seen| pipeline::NeedsView {
                kind: "permission",
                label: None,
                seen,
            }),
            precision: jump::Precision::None,
            animal: "fox".into(),
            simulated: false,
        }
    }

    #[test]
    fn the_strip_rises_for_unseen_knocks_and_fresh_finishes() {
        let t = 1_000_000;
        assert!(rises(&view("needs_input", 0, Some(false)), t));
        assert!(!rises(&view("needs_input", 0, Some(true)), t), "seen");
        assert!(rises(&view("ready", t - 1000, None), t), "just finished");
        let old = t - READY_RISE_MS;
        assert!(!rises(&view("ready", old, None), t), "2 minutes on");
        assert!(!rises(&view("idle", t, None), t), "clicked: idle");
        assert!(!rises(&view("working", t, None), t));
        assert!(!rises(&view("failed", t, None), t));
    }

    #[test]
    fn ago_in_plain_words() {
        let (s, m, h, d) = (1000, 60_000, 3_600_000, 86_400_000);
        assert_eq!(super::ago(0), "just now");
        assert_eq!(super::ago(59 * s), "just now");
        assert_eq!(super::ago(m), "1 minute ago");
        assert_eq!(super::ago(59 * m + 59 * s), "59 minutes ago");
        assert_eq!(super::ago(h), "1 hour ago");
        assert_eq!(super::ago(23 * h), "23 hours ago");
        assert_eq!(super::ago(2 * d), "2 days ago");
    }
}
