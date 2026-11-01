//! Platform layer (SPEC.md §7.3, §9). Every OS call goes through [`Platform`].
//! Stage A has only the Windows implementation.

mod windows;

pub use self::windows::{WindowsPlatform, fill_random, local_minute};
use crate::jump::{ProcessId, Target};
use crate::strip::Rect;

#[derive(Clone, Copy)]
pub struct Taskbar {
    pub rect: Rect,
    /// Screen edge: 0 left, 1 top, 2 right, 3 bottom.
    pub edge: u32,
    pub auto_hide: bool,
}

#[derive(Clone, Copy)]
pub struct Display {
    pub monitor: Rect,
    pub work_area: Rect,
    pub scale: f64,
    pub taskbar: Option<Taskbar>,
}

pub trait Platform: Send + Sync {
    /// The primary monitor's geometry.
    fn primary_display(&self) -> std::io::Result<Display>;
    /// Applies the strip's window styles (§9.1), click-through on, and moves
    /// it topmost to `rect` without activating it.
    fn place_strip(&self, rect: Rect) -> std::io::Result<()>;
    /// Cursor position in physical screen pixels.
    fn cursor_pos(&self) -> Option<(i32, i32)>;
    /// `true`: the strip ignores the mouse (§9.3).
    fn set_click_through(&self, on: bool);
    /// §9.4 step 1: the process that owns the client end of the loopback TCP
    /// connection from `client_port` to Knock's `server_port`.
    fn connection_owner(&self, client_port: u16, server_port: u16) -> Option<ProcessId>;
    /// §9.4 step 2: the window to focus for `process`, if any.
    fn find_window(&self, process: ProcessId, project: &str) -> Option<Target>;
    /// §9.6, ADR 0013: `SetForegroundWindow`, restoring a minimized window.
    /// If Windows refuses, flashes its taskbar button instead. Returns
    /// whether `SetForegroundWindow` succeeded. Never blocks or sleeps.
    fn focus_window(&self, window: isize) -> bool;
    /// §9.7 liveness: the start time of `pid` if a process with that PID is
    /// still running, else `None`.
    fn process_start(&self, pid: u32) -> Option<u64>;
}

/// A recording stand-in for tests: every lookup returns `owner` (or the
/// client port as the PID), every walk finds a Windows Terminal window, and
/// the processes in `alive` are running.
#[cfg(test)]
#[derive(Default)]
pub struct FakePlatform {
    pub lookups: std::sync::Mutex<Vec<(u16, u16)>>,
    pub owner: std::sync::Mutex<Option<ProcessId>>,
    /// The PID of every `find_window` call.
    pub walks: std::sync::Mutex<Vec<u32>>,
    pub alive: std::sync::Mutex<Vec<ProcessId>>,
}

#[cfg(test)]
impl Platform for FakePlatform {
    fn primary_display(&self) -> std::io::Result<Display> {
        unimplemented!()
    }
    fn place_strip(&self, _: Rect) -> std::io::Result<()> {
        unimplemented!()
    }
    fn cursor_pos(&self) -> Option<(i32, i32)> {
        None
    }
    fn set_click_through(&self, _: bool) {}
    fn connection_owner(&self, client: u16, server: u16) -> Option<ProcessId> {
        self.lookups.lock().unwrap().push((client, server));
        Some(self.owner.lock().unwrap().unwrap_or(ProcessId {
            pid: client.into(),
            start: 1,
        }))
    }
    fn find_window(&self, p: ProcessId, _: &str) -> Option<Target> {
        self.walks.lock().unwrap().push(p.pid);
        Some(Target {
            window: 1,
            owner: "WindowsTerminal.exe".into(),
            precision: crate::jump::Precision::AppWindow,
        })
    }
    fn focus_window(&self, _: isize) -> bool {
        false
    }
    fn process_start(&self, pid: u32) -> Option<u64> {
        let alive = self.alive.lock().unwrap();
        alive.iter().find(|p| p.pid == pid).map(|p| p.start)
    }
}
