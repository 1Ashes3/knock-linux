//! The notification-area icon (SPEC.md §9.7): the Needs-you count as a
//! badge (0-9, then 9+) and in the tooltip, and a menu with Pause
//! notifications, where the animals live (ADR 0037), Settings and Quit. The badges are 32 px PNGs made from
//! `art/icon/tray-*.svg` with `pnpm tauri icon <svg> --png 32`.

use crate::pipeline::{self, Msg, Shared};
use crate::settings::Placement;
use std::sync::mpsc::Sender;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Wry};

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

pub fn build(app: &AppHandle, tx: Sender<Msg>, shared: Shared) -> tauri::Result<Tray> {
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
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let line = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&pause, &animals, &fades, &settings, &line, &quit])?;
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
