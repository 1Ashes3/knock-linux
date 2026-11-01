//! The notification-area icon (SPEC.md §9.7): the Needs-you count as a
//! badge (0-9, then 9+) and in the tooltip, and a menu with Pause
//! notifications, Settings and Quit. The badges are 32 px PNGs made from
//! `art/icon/tray-*.svg` with `pnpm tauri icon <svg> --png 32`.

use crate::pipeline::{self, Msg, Shared};
use std::sync::mpsc::Sender;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
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
}

pub fn build(app: &AppHandle, tx: Sender<Msg>, shared: Shared) -> tauri::Result<Tray> {
    let paused = pipeline::read(&shared).settings.pause_notifications;
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
    let menu = Menu::with_items(app, &[&pause, &settings, &line, &quit])?;
    let mut icon = TrayIconBuilder::with_id("knock")
        .tooltip(tooltip(0))
        .menu(&menu)
        .on_menu_event(move |app, e| match e.id().as_ref() {
            "pause" => {
                // From the saved value, not the item: the pipeline sets the
                // check mark back to whatever was saved.
                let on = !pipeline::read(&shared).settings.pause_notifications;
                let patch = serde_json::json!({ "pauseNotifications": on });
                let _ = tx.send(Msg::Settings { patch, reply: None });
            }
            "settings" => tracing::info!("tray: the settings window is not built yet"),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(i) = badge(0).or_else(|| app.default_window_icon().cloned()) {
        icon = icon.icon(i);
    }
    Ok(Tray {
        icon: icon.build(app)?,
        pause,
    })
}

/// The tooltip for `n` Needs-you sessions: the number 0-9, then 9+.
pub fn tooltip(n: usize) -> String {
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
        assert_eq!(tooltip(0), "Knock: 0 sessions need you");
        assert_eq!(tooltip(1), "Knock: 1 session needs you");
        assert_eq!(tooltip(9), "Knock: 9 sessions need you");
        assert_eq!(tooltip(10), "Knock: 9+ sessions need you");
        assert_eq!(tooltip(250), "Knock: 9+ sessions need you");
    }
}
