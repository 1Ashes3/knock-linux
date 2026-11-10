//! The notification-area icon (SPEC.md §9.7): the Needs-you count as a
//! badge (0-9, then 9+) and in the tooltip, and a menu with Pause
//! notifications, where the animals live (ADR 0037), Settings, Introduce
//! sprites (ADR 0040) and Quit. The badges are 32 px PNGs made from
//! `art/icon/tray-*.svg` with `pnpm tauri icon <svg> --png 32`.

use crate::pipeline::{self, Msg, Shared};
use crate::settings::Placement;
use std::sync::mpsc::Sender;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Wry};

/// `icons/tray/tray-<n>.png` for 0-9, then `tray-9plus.png`.
const BADGES: [&[u8]; 11] = [
    include_bytes!("../icons/tray/tray-0.png"),
    include_bytes!("../icons/tray/tray-1.png"),
    include_bytes!("../icons/tray/tray-2.png"),
    include_bytes!("../icons/tray/tray-3.png"),
    include_bytes!("../icons/tray/tray-4.png"),
    include_bytes!("../icons/tray/tray-5.png"),
    include_bytes!("../icons/tray/tray-6.png"),
    include_bytes!("../icons/tray/tray-7.png"),
    include_bytes!("../icons/tray/tray-8.png"),
    include_bytes!("../icons/tray/tray-9.png"),
    include_bytes!("../icons/tray/tray-9plus.png"),
];

/// The badge for `n` Needs-you sessions.
pub fn badge(n: usize) -> Option<Image<'static>> {
    Image::from_bytes(BADGES[n.min(10)]).ok()
}

pub struct Tray {
    pub icon: TrayIcon,
    /// Bound to `pauseNotifications`.
    pub pause: CheckMenuItem<Wry>,
    /// Bound to `placement`: exactly one is checked.
    pub placement: Vec<(Placement, CheckMenuItem<Wry>)>,
    /// Bound to `cardFadeSeconds`: the one matching is checked.
    pub fade: Vec<(u32, CheckMenuItem<Wry>)>,
}

/// The menu id of "Introduce sprites" (ADR 0040).
const INTRO: &str = "intro";

/// How long the "Introduce sprites" demo runs and keeps the strip on top
/// (ADR 0040): the `intro-demo` event's `durationMs` (§8.5).
pub const INTRO_DEMO_MS: u64 = 18_000;

/// The `intro-demo` payload (§8.5).
fn intro_payload() -> serde_json::Value {
    serde_json::json!({ "durationMs": INTRO_DEMO_MS })
}

/// The tray's hover-card fade choices, in seconds.
const FADES: [u32; 4] = [1, 3, 5, 10];

/// The tray's placement items: menu id, setting value, label.
const PLACES: [(&str, Placement, &str, &str); 3] = [
    ("place-top", Placement::Top, "top", "On top of windows"),
    (
        "place-desktop",
        Placement::Desktop,
        "desktop",
        "On the desktop, up front when needed",
    ),
    (
        "place-hidden",
        Placement::Hidden,
        "hidden",
        "Hidden (tray only)",
    ),
];

/// `raise` keeps the strip on top for [`INTRO_DEMO_MS`]; called on each
/// "Introduce sprites" click, before the strip is told.
pub fn build(
    app: &AppHandle,
    tx: Sender<Msg>,
    shared: Shared,
    raise: impl Fn() + Send + Sync + 'static,
) -> tauri::Result<Tray> {
    let raise = std::sync::Arc::new(raise);
    let (paused, placed, faded) = {
        let s = &pipeline::read(&shared).settings;
        (s.pause_notifications, s.placement, s.card_fade_seconds)
    };
    let fade = FADES
        .iter()
        .map(|&n| {
            let (id, label) = (
                format!("fade-{n}"),
                format!("{n} second{}", if n == 1 { "" } else { "s" }),
            );
            CheckMenuItem::with_id(app, id, label, true, n == faded, None::<&str>)
                .map(|item| (n, item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let fade_items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = fade
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let fades = Submenu::with_items(app, "Card fades after", true, &fade_items)?;
    let placement = PLACES
        .iter()
        .map(|(id, p, _, label)| {
            CheckMenuItem::with_id(app, *id, *label, true, *p == placed, None::<&str>)
                .map(|item| (*p, item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = placement
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let animals = Submenu::with_items(app, "Sprites", true, &items)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        "Pause notifications",
        true,
        paused,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let intro = MenuItem::with_id(app, INTRO, "Introduce sprites", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let line = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&pause, &animals, &fades, &settings, &intro, &line, &quit],
    )?;
    let mut icon = TrayIconBuilder::with_id("knock")
        .tooltip(tooltip(0, false))
        .menu(&menu)
        .on_menu_event(move |app, e| match e.id().as_ref() {
            // The pipeline flips its in-effect value and sets the check
            // mark to the result.
            "pause" => {
                let _ = tx.send(Msg::TogglePause);
            }
            "settings" => {
                // Off the main thread: creating a window in an event
                // handler deadlocks on Windows.
                let app = app.clone();
                std::thread::spawn(move || crate::open_settings(&app, ""));
            }
            // Raises the strip and tells it to run its demo (ADR 0040); no
            // setting changes. Off the main thread like Settings.
            INTRO => {
                let (app, raise) = (app.clone(), std::sync::Arc::clone(&raise));
                std::thread::spawn(move || {
                    raise();
                    let _ = app.emit_to("strip", "intro-demo", intro_payload());
                });
            }
            "quit" => app.exit(0),
            // Saved like any setting; `settings-changed` then sets the
            // check marks to what was saved.
            id => {
                let patch = if let Some((_, _, value, _)) = PLACES.iter().find(|(i, ..)| *i == id) {
                    serde_json::json!({ "placement": value })
                } else if let Some(n) = id.strip_prefix("fade-").and_then(|n| n.parse::<u32>().ok())
                {
                    serde_json::json!({ "cardFadeSeconds": n })
                } else {
                    return;
                };
                let _ = tx.send(Msg::Settings { patch, reply: None });
            }
        });
    if let Some(i) = badge(0).or_else(|| app.default_window_icon().cloned()) {
        icon = icon.icon(i);
    }
    Ok(Tray {
        icon: icon.build(app)?,
        pause,
        placement,
        fade,
    })
}

/// The tooltip for `n` Needs-you sessions: the number 0-9, then 9+. While
/// the server is down (`down`), the Repair hint instead (ADR 0023).
pub fn tooltip(n: usize, down: bool) -> String {
    if down {
        return "Knock can't listen for Claude Code. Open Settings to repair.".into();
    }
    match n {
        1 => "Knock: 1 session needs you".into(),
        0..=9 => format!("Knock: {n} sessions need you"),
        _ => "Knock: 9+ sessions need you".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_badge_is_a_32_px_image_and_ten_up_is_nine_plus() {
        for n in 0..=10 {
            let b = badge(n).unwrap();
            assert_eq!((b.width(), b.height()), (32, 32), "{n}");
        }
        assert_eq!(badge(10).unwrap().rgba(), badge(250).unwrap().rgba());
        assert_ne!(badge(9).unwrap().rgba(), badge(10).unwrap().rgba());
        assert_ne!(badge(0).unwrap().rgba(), badge(1).unwrap().rgba());
    }

    #[test]
    fn intro_id_is_not_taken_by_a_setting_item() {
        assert!(PLACES.iter().all(|(id, ..)| *id != INTRO));
        assert!(INTRO.strip_prefix("fade-").is_none());
        assert!(!["pause", "settings", "quit"].contains(&INTRO));
    }

    #[test]
    fn intro_demo_payload_carries_its_duration() {
        assert_eq!(INTRO_DEMO_MS, 18_000);
        assert_eq!(intro_payload(), serde_json::json!({ "durationMs": 18000 }));
    }

    #[test]
    fn tooltip_counts_to_nine_then_nine_plus() {
        assert_eq!(tooltip(0, false), "Knock: 0 sessions need you");
        assert_eq!(tooltip(1, false), "Knock: 1 session needs you");
        assert_eq!(tooltip(9, false), "Knock: 9 sessions need you");
        assert_eq!(tooltip(10, false), "Knock: 9+ sessions need you");
        assert_eq!(tooltip(250, false), "Knock: 9+ sessions need you");
        assert_eq!(
            tooltip(3, true),
            "Knock can't listen for Claude Code. Open Settings to repair."
        );
    }
}
