//! Knock desktop shell. Phase 0: the overlay strip (Spike A, SPEC.md §9.1-9.3),
//! the hook server (Spike B, §8.1), jumping to a session (Spike C, §9.4-9.6)
//! and, in debug builds, the toast click spike (V-12, §9.7).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod jump;
mod log;
mod platform;
mod server;
mod sessions;
mod strip;
#[cfg(debug_assertions)]
mod toast;

use platform::{Platform, WindowsPlatform};
use sessions::{SessionView, Sessions};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use strip::{MEDIUM_HEIGHT, Region};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Spike A: fixed width of 3 slots (§9.2); `request_strip_size` comes later.
const SLOTS: u32 = 3;
/// About 30 cursor reads a second (§9.3).
const POLL: Duration = Duration::from_millis(33);

#[derive(Default)]
struct HitRegions(Arc<Mutex<Vec<Region>>>);

/// §8.5: replaces the strip's hit regions; empty means fully click-through.
#[tauri::command]
fn set_hit_regions(regions: Vec<Region>, state: tauri::State<'_, HitRegions>) {
    *state.0.lock().unwrap() = regions;
}

/// §8.5: sessions, most recent event first.
#[tauri::command]
fn get_sessions(sessions: tauri::State<'_, Arc<Sessions>>) -> Vec<SessionView> {
    sessions.views()
}

/// §8.5, §9.5, §9.6: focuses the session's window. An unknown id, or a session
/// with no known process or window, is a no-op. Sync, so it runs on the main
/// thread, the one that received the click. `user.ack` comes with the engine.
#[tauri::command]
fn jump_to_session(
    id: String,
    sessions: tauri::State<'_, Arc<Sessions>>,
    platform: tauri::State<'_, Arc<dyn Platform>>,
) {
    jump(&sessions, &**platform, &id, "strip");
}

/// `jump_to_session`'s work; `via` names what was clicked, for the log.
fn jump(sessions: &Sessions, platform: &dyn Platform, id: &str, via: &str) {
    let Some((process, project, _)) = sessions.jump_info(id) else {
        tracing::info!(session = ?id, via, "jump: no process");
        return;
    };
    let t = Instant::now();
    // Looked up again on every jump: the window may have closed or moved.
    let target = platform.find_window(process, &project);
    let Some(target) = target else {
        tracing::info!(session = ?id, via, "jump: no window");
        return;
    };
    let front = platform.focus_window(target.window);
    tracing::info!(
        session = ?id,
        via,
        precision = ?target.precision,
        front,
        flashed = !front,
        took_us = t.elapsed().as_micros() as u64,
        "jump"
    );
}

fn main() {
    log::init();
    let sessions = Arc::new(Sessions::default());
    let builder = tauri::Builder::default();
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_notification::init());
    builder
        .manage(HitRegions::default())
        .manage(Arc::clone(&sessions))
        .invoke_handler(tauri::generate_handler![
            set_hit_regions,
            get_sessions,
            jump_to_session
        ])
        .setup(move |app| {
            // Created visible but unfocused, so tao shows it with
            // SW_SHOWNOACTIVATE. Never call anything that focuses it (§9.1).
            let url = WebviewUrl::App("/overlay/index.html".into());
            let window = WebviewWindowBuilder::new(app, "strip", url)
                .title("Knock")
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
            app.manage(Arc::clone(&platform));
            #[cfg(debug_assertions)]
            toast::init(app.handle());
            // The Repair flow for a port that can't bind is not built yet (§8.1).
            if let Err(e) = server::start(Arc::clone(&platform), sessions) {
                tracing::error!(kind = ?e.kind(), "server failed to start");
            }

            let display = platform.primary_display()?;
            let rect = strip::place(display.work_area, display.scale, MEDIUM_HEIGHT, SLOTS);
            // Spike logging for V-13: geometry only, no paths or agent data.
            eprintln!(
                "knock: monitor {:?} work area {:?} scale {}",
                display.monitor, display.work_area, display.scale
            );
            match display.taskbar {
                Some(t) => eprintln!(
                    "knock: taskbar {:?} edge {} auto-hide {}",
                    t.rect, t.edge, t.auto_hide
                ),
                None => eprintln!("knock: taskbar not found"),
            }
            eprintln!("knock: strip {rect:?}");
            platform.place_strip(rect)?;

            let regions = Arc::clone(&app.state::<HitRegions>().0);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(POLL);
                    let inside = platform.cursor_pos().is_some_and(|c| {
                        strip::hit(&regions.lock().unwrap(), rect, display.scale, c)
                    });
                    // Every tick, so styles tao rewrites are repaired within
                    // one tick (ADR 0001). It writes only when they differ.
                    platform.set_click_through(!inside);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Knock");
}
