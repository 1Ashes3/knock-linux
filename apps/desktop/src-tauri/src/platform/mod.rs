//! Platform layer (SPEC.md §7.3, §9). Every OS call goes through [`Platform`].
//! Stage A has only the Windows implementation.

mod windows;

pub use self::windows::{WindowsPlatform, fill_random, link_count, local_minute};
use crate::jump::{ProcessId, Target};
use crate::strip::Rect;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taskbar {
    pub rect: Rect,
    /// Screen edge: 0 left, 1 top, 2 right, 3 bottom.
    pub edge: u32,
    pub auto_hide: bool,
}

/// One display, for the setup step that picks a taskbar (§10.4).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Monitor {
    /// Stable across restarts and reconnects: a hash of the monitor's
    /// device interface path (see [`monitor_id`]). Used as §8.6 `monitor`.
    pub id: String,
    pub name: String,
    pub primary: bool,
    /// Resolution in physical pixels.
    pub width: i32,
    pub height: i32,
}

/// §8.6 `monitor` id for a monitor's device interface path (stable for the
/// same monitor on the same port): the same 64-bit FNV-1a as the project
/// key, as 16 hex digits, so it fits the §8.6 id rule. Device paths are
/// case-insensitive, so the lower-casing is harmless.
pub fn monitor_id(device_path: &str) -> String {
    knock_core::project_key(device_path)
}

/// One display's geometry, for placing the strip (§9.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// The same id as [`Monitor::id`]: the §8.6 `monitor` value.
    pub id: String,
    pub primary: bool,
    pub monitor: Rect,
    pub work_area: Rect,
    pub scale: f64,
}

pub trait Platform: Send + Sync {
    /// Every connected display's geometry, the primary one first.
    fn displays(&self) -> Vec<Display>;
    /// The primary taskbar as the shell reports it (ABM_GETTASKBARPOS: its
    /// shown position, even while auto-hidden; ADR 0002).
    fn taskbar(&self) -> Option<Taskbar>;
    /// Where the strip is now, in physical screen pixels.
    fn strip_rect(&self) -> Option<Rect>;
    /// Sets `changed` whenever the strip's window hears of a display, DPI,
    /// work-area or taskbar change (FR-15). Call once, on the main thread.
    fn watch_displays(&self, changed: Arc<AtomicBool>);
    /// Applies the strip's window styles (§9.1), click-through on, and moves
    /// it topmost to `rect` without activating it.
    fn place_strip(&self, rect: Rect) -> std::io::Result<()>;
    /// Shows the strip without activating it, then re-asserts its styles.
    /// The strip is created hidden and shown only after `place_strip`, so it
    /// is never visible without its §9.1 styles (ADR 0001).
    fn show_strip(&self);
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
    /// Every connected display, the primary one first.
    fn monitors(&self) -> Vec<Monitor>;
    /// §9.7: adds or removes Knock's launch-at-sign-in entry, the per-user
    /// `HKCU\...\Run\Knock` value only (per-user install; never HKLM), as
    /// [`sign_in_ops`] says.
    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<()>;
}

/// The `Run\Knock` value: the exe's path, quoted, so a path with spaces
/// can't be read as a shorter program plus arguments.
pub fn run_value(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

/// One change to the sign-in entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOp {
    /// Write `Run\Knock` with Knock's own value.
    Write,
    /// Delete `Run\Knock`.
    Delete,
    /// Delete `StartupApproved\Run\Knock`, so Task Manager doesn't keep
    /// showing Knock as disabled. No value there means enabled.
    ClearApproved,
}

/// What turning sign-in `on` or off does, given the current `Run\Knock`
/// string data (`None`: no value, or not a string) and Knock's `own` value
/// ([`run_value`]). On: always overwrite (the user asked; `current` is not
/// needed) and clear a "disabled" mark. Off: delete a value that starts
/// Knock ([`starts_knock`]); one that starts another program stays.
pub fn sign_in_ops(on: bool, current: Option<&str>, own: &str) -> &'static [RunOp] {
    match on {
        true => &[RunOp::Write, RunOp::ClearApproved],
        false if current.is_some_and(|c| starts_knock(c, own)) => {
            &[RunOp::Delete, RunOp::ClearApproved]
        }
        false => &[],
    }
}

/// `Run\Knock` data is Knock's: its `own` value, or a command whose program
/// is named `Knock.exe` (any letter case, any folder, quoted or not, with
/// or without arguments), since an older copy of Knock may have written it.
/// An unquoted program ends at the first `.exe` that ends a word.
fn starts_knock(data: &str, own: &str) -> bool {
    let d = data.trim_start();
    let program = match d.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => {
            // ASCII lowercasing keeps every byte index.
            let l = d.to_ascii_lowercase();
            let end = (l.match_indices(".exe").map(|(i, _)| i + 4))
                .find(|&e| l[e..].chars().next().is_none_or(char::is_whitespace));
            end.map_or(d, |e| &d[..e])
        }
    };
    let name = program.rsplit(['\\', '/']).next().unwrap_or("");
    data.eq_ignore_ascii_case(own) || name.eq_ignore_ascii_case("Knock.exe")
}

/// Whether to change the real sign-in entry. A debug build does only with
/// `KNOCK_AUTOSTART_TEST=1`, so a development run never leaves one behind.
pub fn sign_in_allowed(debug: bool, test_var: Option<&std::ffi::OsStr>) -> bool {
    !debug || test_var.is_some_and(|v| v == "1")
}

/// A recording stand-in for tests: every lookup returns `owner` (or the
/// client port as the PID), every walk finds a Windows Terminal window, the
/// processes in `alive` are running, and the sign-in entry lives in `run`
/// and `approved` instead of the registry.
#[cfg(test)]
#[derive(Default)]
pub struct FakePlatform {
    pub lookups: std::sync::Mutex<Vec<(u16, u16)>>,
    pub owner: std::sync::Mutex<Option<ProcessId>>,
    /// The PID of every `find_window` call.
    pub walks: std::sync::Mutex<Vec<u32>>,
    pub alive: std::sync::Mutex<Vec<ProcessId>>,
    /// Every `set_launch_at_sign_in` call.
    pub sign_in: std::sync::Mutex<Vec<bool>>,
    /// `set_launch_at_sign_in` fails while set.
    pub sign_in_fails: std::sync::Mutex<bool>,
    /// The fake `Run\Knock` and `StartupApproved\Run\Knock` values.
    pub run: std::sync::Mutex<Option<String>>,
    pub approved: std::sync::Mutex<Option<Vec<u8>>>,
}

/// The fake's own `Run\Knock` value.
#[cfg(test)]
pub const FAKE_EXE: &str = r#""C:\Users\A B\AppData\Local\Knock\Knock.exe""#;

#[cfg(test)]
impl Platform for FakePlatform {
    fn displays(&self) -> Vec<Display> {
        Vec::new()
    }
    fn taskbar(&self) -> Option<Taskbar> {
        None
    }
    fn strip_rect(&self) -> Option<Rect> {
        None
    }
    fn watch_displays(&self, _: Arc<AtomicBool>) {}
    fn place_strip(&self, _: Rect) -> std::io::Result<()> {
        unimplemented!()
    }
    fn show_strip(&self) {}
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
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<()> {
        self.sign_in.lock().unwrap().push(on);
        if *self.sign_in_fails.lock().unwrap() {
            return Err(std::io::Error::other("refused"));
        }
        let mut run = self.run.lock().unwrap();
        for op in sign_in_ops(on, run.as_deref(), FAKE_EXE) {
            match op {
                RunOp::Write => *run = Some(FAKE_EXE.into()),
                RunOp::Delete => *run = None,
                RunOp::ClearApproved => *self.approved.lock().unwrap() = None,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_value_is_the_quoted_exe_path() {
        let exe = std::path::Path::new(r"C:\Users\A B\AppData\Local\Knock\Knock.exe");
        assert_eq!(run_value(exe), FAKE_EXE);
        // Through the trait, a fake records the call and touches nothing.
        let fake = FakePlatform::default();
        let p: &dyn Platform = &fake;
        p.set_launch_at_sign_in(true).unwrap();
        p.set_launch_at_sign_in(false).unwrap();
        assert_eq!(*fake.sign_in.lock().unwrap(), [true, false]);
    }

    /// StartupApproved's value after the user disabled Knock in Task
    /// Manager: 03 00 00 00, then a FILETIME.
    const DISABLED: [u8; 12] = [3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

    #[test]
    fn enabling_overwrites_any_value_and_clears_a_disabled_mark() {
        let fake = FakePlatform::default();
        *fake.run.lock().unwrap() = Some(r#""C:\Other\Knock.exe" --x"#.into());
        *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
        fake.set_launch_at_sign_in(true).unwrap();
        assert_eq!(fake.run.lock().unwrap().as_deref(), Some(FAKE_EXE));
        assert_eq!(
            *fake.approved.lock().unwrap(),
            None,
            "Task Manager: enabled"
        );
        // Already ours and enabled: the same result.
        fake.set_launch_at_sign_in(true).unwrap();
        assert_eq!(fake.run.lock().unwrap().as_deref(), Some(FAKE_EXE));
        assert_eq!(*fake.approved.lock().unwrap(), None);
    }

    #[test]
    fn disabling_deletes_only_a_value_that_starts_knock() {
        let fake = FakePlatform::default();
        let other = r#""C:\Other\NotKnock.exe""#;
        *fake.run.lock().unwrap() = Some(other.into());
        *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
        fake.set_launch_at_sign_in(false).unwrap();
        assert_eq!(fake.run.lock().unwrap().as_deref(), Some(other), "not ours");
        assert_eq!(
            fake.approved.lock().unwrap().as_deref(),
            Some(&DISABLED[..])
        );
        // Ours, in any letter case: both values go.
        *fake.run.lock().unwrap() = Some(FAKE_EXE.to_uppercase());
        fake.set_launch_at_sign_in(false).unwrap();
        assert_eq!(*fake.run.lock().unwrap(), None);
        assert_eq!(*fake.approved.lock().unwrap(), None);
        // Nothing there: nothing to do.
        fake.set_launch_at_sign_in(false).unwrap();
        assert_eq!(*fake.run.lock().unwrap(), None);
        assert_eq!(sign_in_ops(false, None, FAKE_EXE), []);
    }

    #[test]
    fn any_command_starting_a_knock_exe_is_knocks() {
        let off = |data: &str| sign_in_ops(false, Some(data), FAKE_EXE);
        let delete = [RunOp::Delete, RunOp::ClearApproved];
        for ours in [
            FAKE_EXE,
            FAKE_EXE.trim_matches('"'),
            r#""C:\Old\Knock.exe""#,
            r"C:\Old\Knock.exe",
            r#""C:\Program Files\Knock\Knock.exe" --minimized"#,
            r"C:\Program Files\Knock\Knock.exe --minimized",
            "C:\\Old\\Knock.exe\t-x",
            r#"  "C:\OLD\KNOCK.EXE""#,
            "c:/old/knock.Exe",
            "Knock.exe",
            r#""C:\Old\Knock.exe"#,
        ] {
            assert_eq!(off(ours), delete, "{ours}");
        }
        for theirs in [
            "",
            "   ",
            r#""C:\Old\NotKnock.exe""#,
            r"C:\Old\NotKnock.exe --knock",
            r"C:\Old\Knock.exe.bak",
            r#""C:\Old\Knock.exe.bak""#,
            r#""C:\Old\Other.exe" C:\Old\Knock.exe"#,
            r"C:\Tools\Other.exe C:\Old\Knock.exe",
            r"C:\Knock.exe\Other.exe",
            r"C:\Old\Knock",
        ] {
            assert_eq!(off(theirs), [], "{theirs}");
        }
        // Turning on never looks at the value.
        for current in [None, Some(""), Some("anything")] {
            let on = sign_in_ops(true, current, FAKE_EXE);
            assert_eq!(on, [RunOp::Write, RunOp::ClearApproved]);
        }
    }

    #[test]
    fn a_debug_build_changes_the_sign_in_entry_only_when_asked() {
        let one = Some(std::ffi::OsStr::new("1"));
        assert!(sign_in_allowed(false, None));
        assert!(sign_in_allowed(false, Some("0".as_ref())));
        assert!(!sign_in_allowed(true, None));
        assert!(!sign_in_allowed(true, Some("true".as_ref())));
        assert!(sign_in_allowed(true, one));
    }

    #[test]
    fn monitor_id_is_stable_and_fits_the_settings_rule() {
        // The shape EnumDisplayDevicesW returns with EDD_GET_DEVICE_INTERFACE_NAME.
        let path =
            r"\\?\DISPLAY#VSC9C2E#5&1a2b3c4d&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";
        let id = super::monitor_id(path);
        assert_eq!(id, super::monitor_id(&path.to_uppercase()));
        assert_eq!(
            id, "ce99b1e1b13dd0b5",
            "changing it forgets the chosen taskbar"
        );
        let s = crate::settings::Settings::default();
        assert!(crate::settings::merged(&s, &serde_json::json!({ "monitor": id })).is_ok());
    }
}
