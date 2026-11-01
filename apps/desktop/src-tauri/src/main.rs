//! Knock desktop shell (SPEC.md §7.1, §7.2): the strip window (§9.1-9.3), the
//! local server (§8.1), the pipeline and its effect runner, the tray (§9.7)
//! and the §8.5 commands and events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hooks;
mod jump;
mod log;
mod pipeline;
mod platform;
mod server;
mod settings;
mod strip;
mod tray;

use pipeline::{Msg, Out, SessionsChanged, Shared};
use platform::{Monitor, Platform, WindowsPlatform};
use serde_json::{Value, json};
use settings::Settings;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use strip::{Region, Strip};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

/// Width of the strip, in slots, until the UI asks for a size.
const SLOTS: f64 = 3.0;
/// About 30 cursor reads a second (§9.3).
const POLL: Duration = Duration::from_millis(33);

/// What the commands share. Reads come from [`Shared`]; changes go to the
/// pipeline's owner as messages, so no command waits on a lock it holds.
struct Knock {
    tx: Sender<Msg>,
    shared: Shared,
    platform: Arc<dyn Platform>,
    strip: Arc<Mutex<Strip>>,
    /// Claude Code hook setup (§8.2).
    hooks: hooks::Setup,
    /// No Knock `settings.json` existed when the app started.
    first_run: bool,
}

impl Knock {
    fn strip(&self) -> std::sync::MutexGuard<'_, Strip> {
        self.strip.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// §8.5: the current `{ rev, sessions }`, as in `sessions-changed`.
#[tauri::command]
fn get_sessions(k: tauri::State<'_, Knock>) -> SessionsChanged {
    pipeline::read(&k.shared).sessions.clone()
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

/// §8.5, ADR 0003: replaces the strip's hit regions; empty means fully
/// click-through.
#[tauri::command]
fn set_hit_regions(regions: Vec<Region>, k: tauri::State<'_, Knock>) {
    k.strip().regions = regions;
}

/// §8.5, §9.2: resizes the strip, keeping its bottom-right corner, at most 6
/// slots wide. Width 0 (no sessions) leaves a zero-width strip. Never
/// activates it (§9.1). Primary monitor only, for now.
#[tauri::command]
fn request_strip_size(width: f64, height: f64, k: tauri::State<'_, Knock>) -> Result<(), String> {
    let slot = pipeline::read(&k.shared).settings.size.height();
    let d = k.platform.primary_display().map_err(|e| {
        tracing::warn!(kind = ?e.kind(), "display not found");
        "Knock couldn't find the display.".to_string()
    })?;
    let max_height = f64::from(d.work_area.bottom - d.work_area.top) / d.scale;
    let (w, h) = strip::clamp(width, height, slot, max_height)
        .ok_or("width and height must be numbers of 0 or more")?;
    let rect = strip::place(d.work_area, d.scale, w, h);
    k.platform.place_strip(rect).map_err(|e| {
        tracing::warn!(kind = ?e.kind(), "strip not placed");
        "Knock couldn't place the strip.".to_string()
    })?;
    let mut s = k.strip();
    (s.rect, s.scale) = (rect, d.scale);
    Ok(())
}

/// §8.6.
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
    let Some((port, token)) = k.hooks.server.clone() else {
        return false;
    };
    let probe = move || server::probe(port, &token);
    tauri::async_runtime::spawn_blocking(probe)
        .await
        .unwrap_or(false)
}

/// §8.5, §10.4.
#[tauri::command]
async fn setup_status(k: tauri::State<'_, Knock>) -> Result<Value, String> {
    Ok(json!({
        "firstRun": k.first_run,
        "hooksInstalled": k.hooks.installed().unwrap_or(false),
        "serverOk": server_ok(&k).await,
        "monitorCount": k.platform.monitors().len(),
    }))
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
    let hooks = match k.hooks.installed() {
        Ok(true) => (true, "Knock is in your Claude Code settings (settings.json).".into()),
        Ok(false) => (
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
            "Knock isn't answering Claude Code right now. Please restart Knock.".into(),
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

/// Opens the settings window, or brings it back if it is open. `setup`:
/// start the setup flow (§10.4). A normal, focusable window, unlike the
/// strip. From an event handler, call it off the main thread: creating a
/// window there deadlocks on Windows.
fn open_settings(app: &AppHandle, setup: bool) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = (w.unminimize(), w.show(), w.set_focus());
        return;
    }
    let url = match setup {
        true => "/settings/index.html?setup=1",
        false => "/settings/index.html",
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
fn apply(app: &AppHandle, tray: &tray::Tray, out: Out) {
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
            let _ = app.emit_to("strip", "sessions-changed", payload);
        }
        Out::Settings(settings) => {
            let (item, on) = (tray.pause.clone(), settings.pause_notifications);
            let _ = app.run_on_main_thread(move || {
                let _ = item.set_checked(on);
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
                let _ = icon.set_tooltip(Some(tray::tooltip(n)));
            });
        }
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Created hidden; shown by the platform layer only once its §9.1 styles
    // and position are applied (below). Never call anything that focuses it.
    let url = WebviewUrl::App("/overlay/index.html".into());
    let window = WebviewWindowBuilder::new(app, "strip", url)
        .title("Knock")
        .visible(false)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .resizable(false)
        .shadow(false)
        .no_redirection_bitmap(true)
        .build()?;
    let platform: Arc<dyn Platform> = Arc::new(WindowsPlatform::new(&window)?);

    let dir = settings::data_dir()?;
    let first_run = !dir.join(settings::FILE).exists();
    let shared = Shared::default();
    let now = pipeline::now_ms();
    let core = pipeline::Core::new(
        dir.clone(),
        Some(Arc::clone(&platform)),
        shared.clone(),
        now,
        platform::local_minute(),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let tray = tray::build(app.handle(), tx.clone(), shared.clone())?;
    let handle = app.handle().clone();
    std::thread::Builder::new()
        .name("knock-pipeline".into())
        .spawn(move || pipeline::run(core, rx, |out| apply(&handle, &tray, out)))?;

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

    // The Repair flow for a port that can't bind is not built yet (§8.1).
    let server = match server::start(&dir, Arc::clone(&platform), tx.clone(), shared.clone()) {
        Ok(cfg) => Some((cfg.port, cfg.token)),
        Err(e) => {
            tracing::error!(kind = ?e.kind(), "server failed to start");
            None
        }
    };
    let spec = knock_core::Adapter::hooks(&knock_core::ClaudeCode { users: vec![] });
    let claude = hooks::claude_settings();
    tracing::info!(
        path_set = claude.is_some(),
        test_file = cfg!(debug_assertions) && claude.is_some(),
        "claude settings file"
    );
    let hooks = hooks::Setup::new(spec, claude, dir.join("backups"), server);

    let d = platform.primary_display()?;
    tracing::info!(
        monitor = ?d.monitor,
        work_area = ?d.work_area,
        scale = d.scale,
        taskbar = ?d.taskbar.map(|t| t.rect),
        edge = d.taskbar.map(|t| t.edge),
        auto_hide = d.taskbar.map(|t| t.auto_hide),
        "display"
    );
    let slot = pipeline::read(&shared).settings.size.height();
    let rect = strip::place(d.work_area, d.scale, SLOTS * slot, slot);
    platform.place_strip(rect)?;
    platform.show_strip();
    let strip = Arc::new(Mutex::new(Strip {
        rect,
        scale: d.scale,
        regions: Vec::new(),
    }));

    let (p, s) = (Arc::clone(&platform), Arc::clone(&strip));
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(POLL);
            let inside = p.cursor_pos().is_some_and(|c| {
                let s = s.lock().unwrap_or_else(|e| e.into_inner());
                strip::hit(&s.regions, s.rect, s.scale, c)
            });
            // Every tick, so styles tao rewrites are repaired within one
            // tick (ADR 0001). It writes only when they differ.
            p.set_click_through(!inside);
        }
    });

    // §10.4: setup opens by itself on first run, or while Knock isn't
    // connected to Claude Code.
    let installed = hooks.installed().unwrap_or(false);
    tracing::info!(first_run, installed, "setup status");
    app.manage(Knock {
        tx,
        shared,
        platform,
        strip,
        hooks,
        first_run,
    });
    if first_run || !installed {
        open_settings(app.handle(), true);
    }
    Ok(())
}

fn main() {
    log::init();
    tauri::Builder::default()
        // First, so a second launch exits before anything else starts (§9.7).
        // Its arguments and folder are not logged (§11.5).
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {
            tracing::info!("second launch ignored");
        }))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            jump_to_session,
            acknowledge,
            dismiss,
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
        ])
        .setup(setup)
        .run(tauri::generate_context!())
        .expect("error while running Knock");
}

#[cfg(test)]
mod tests {
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
