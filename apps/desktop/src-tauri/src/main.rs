//! Knock desktop shell. Phase 0 Spike A: the overlay strip (SPEC.md §9.1-9.3).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;
mod strip;

use platform::{Platform, WindowsPlatform};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

fn main() {
    tauri::Builder::default()
        .manage(HitRegions::default())
        .invoke_handler(tauri::generate_handler![set_hit_regions])
        .setup(|app| {
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

            let platform = WindowsPlatform::new(&window)?;
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
