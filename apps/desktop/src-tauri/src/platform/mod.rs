//! Platform layer (SPEC.md §7.3, §9). Every OS call goes through [`Platform`].
//! Stage A has only the Windows implementation.

mod windows;

pub use self::windows::WindowsPlatform;
use crate::strip::Rect;

/// Taskbar geometry, logged for V-13.
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
}
