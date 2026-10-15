//! Windows implementation of [`Platform`]. All Win32 calls live here.

use super::{Display, Platform, Taskbar};
use crate::strip::Rect;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

pub struct WindowsPlatform {
    // HWND is !Send; the raw value is safe to use from any thread for the
    // style and position calls below.
    strip: isize,
}

impl WindowsPlatform {
    pub fn new(strip: &tauri::WebviewWindow) -> tauri::Result<Self> {
        Ok(Self {
            strip: strip.hwnd()?.0 as isize,
        })
    }

    fn hwnd(&self) -> HWND {
        HWND(self.strip as _)
    }
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

impl Platform for WindowsPlatform {
    fn primary_display(&self) -> std::io::Result<Display> {
        // SAFETY: plain Win32 queries with valid, initialized out-params.
        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(mon, &mut info).as_bool() {
                return Err(std::io::Error::last_os_error());
            }
            let (mut dpi_x, mut dpi_y) = (0, 0);
            GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)?;

            let mut bar = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                ..Default::default()
            };
            let taskbar = (SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) != 0).then(|| Taskbar {
                rect: rect(bar.rc),
                edge: bar.uEdge,
                auto_hide: SHAppBarMessage(ABM_GETSTATE, &mut bar) & ABS_AUTOHIDE as usize != 0,
            });

            Ok(Display {
                monitor: rect(info.rcMonitor),
                work_area: rect(info.rcWork),
                scale: f64::from(dpi_x) / 96.0,
                taskbar,
            })
        }
    }

    fn place_strip(&self, r: Rect) -> std::io::Result<()> {
        self.set_click_through(true);
        // SAFETY: valid window handle owned by this process.
        unsafe {
            SetWindowPos(
                self.hwnd(),
                Some(HWND_TOPMOST),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                // FRAMECHANGED makes the new extended styles take effect.
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )?;
        }
        Ok(())
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: valid out-param.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x, p.y))
    }

    fn set_click_through(&self, on: bool) {
        // Writes the whole set every time, so the strip's styles self-heal if
        // anything else rewrote them. LAYERED stays on permanently; only
        // TRANSPARENT toggles, which avoids recreating the layered surface.
        let keep = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED).0 as isize;
        let drop = WS_EX_APPWINDOW.0 as isize;
        let transparent = WS_EX_TRANSPARENT.0 as isize;
        // SAFETY: valid window handle owned by this process.
        unsafe {
            let old = GetWindowLongPtrW(self.hwnd(), GWL_EXSTYLE);
            let mut new = (old | keep) & !drop;
            if on {
                new |= transparent;
            } else {
                new &= !transparent;
            }
            if new != old {
                SetWindowLongPtrW(self.hwnd(), GWL_EXSTYLE, new);
            }
        }
    }
}
