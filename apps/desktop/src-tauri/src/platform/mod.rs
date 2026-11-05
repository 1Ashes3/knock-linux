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
    /// `HKCU\...\Run\Knock` value only (per-user install; never HKLM): the
    /// `now` part of [`sign_in_change`]. Returns the whole change, so the
    /// caller applies its `saved` part once the setting is saved, or its
    /// `undo` part if that fails ([`Platform::apply_sign_in_ops`]).
    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<SignInChange>;
    /// Applies `ops` from a [`SignInChange`] to the sign-in entry.
    fn apply_sign_in_ops(&self, ops: &[RunOp]) -> std::io::Result<()>;
    /// ADR 0027, at app start: rewrites the entry as [`start_ops`] says for
    /// the `launchAtSignIn` `setting`. Returns whether it wrote.
    fn restore_launch_at_sign_in(&self, setting: Option<bool>) -> std::io::Result<bool>;
    /// §9.7 idle time: ms since the user last used the mouse, keyboard or
    /// touch anywhere in this Windows session, or `None` if it can't be read.
    fn idle_ms(&self) -> Option<u64>;
}

/// §9.7 idle time from the system tick count now (`GetTickCount64`) and the
/// last input's tick (`GetLastInputInfo`), which is only 32 bits and wraps
/// every 49.7 days. The low 32 bits of `now` minus `last_input`, wrapping,
/// is exact while the idle time is under 49.7 days.
pub fn idle_from_ticks(now: u64, last_input: u32) -> u64 {
    u64::from((now as u32).wrapping_sub(last_input))
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

/// One change of the `launchAtSignIn` setting to the sign-in entry, in the
/// order the setting is saved: `now` before the save, `saved` after it
/// succeeds, `undo` if it fails and the setting was different.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignInChange {
    pub now: &'static [RunOp],
    pub saved: &'static [RunOp],
    pub undo: &'static [RunOp],
}

/// What turning sign-in `on` or off does, given the current `Run\Knock`
/// string data (`None`: no value, or not a string; an expandable string
/// unexpanded) and Knock's `own` value ([`run_value`] of the running exe).
/// On: always overwrite (the user asked; `current` is not needed) and clear
/// a "disabled" mark; undone by deleting the value again. Off: delete a
/// value that starts Knock's app ([`starts_knock`]) and clear the mark; one
/// that starts another program stays (ADR 0025). The mark is cleared only
/// once the setting is saved, so an undone turn-off writes the value back
/// (as at start, ADR 0027) and the user's Task Manager mark is as it was.
pub fn sign_in_change(on: bool, current: Option<&str>, own: &str) -> SignInChange {
    match on {
        true => SignInChange {
            now: &[RunOp::Write, RunOp::ClearApproved],
            saved: &[],
            undo: &[RunOp::Delete, RunOp::ClearApproved],
        },
        false if current.is_some_and(|c| starts_knock(c, own)) => SignInChange {
            now: &[RunOp::Delete],
            saved: &[RunOp::ClearApproved],
            undo: &[RunOp::Write],
        },
        false => SignInChange::default(),
    }
}

/// ADR 0027: what Knock does at start, given the `launchAtSignIn` setting,
/// the current `Run\Knock` data (as for [`sign_in_change`]) and Knock's `own`
/// value. On: anything but Knock's own value (missing, an old path, another
/// program's) is rewritten; letter case doesn't count, as paths ignore it.
/// The Task Manager mark stays: only turning the setting on clears it.
/// Off or unset: nothing.
pub fn start_ops(setting: Option<bool>, current: Option<&str>, own: &str) -> &'static [RunOp] {
    match (setting, current) {
        (Some(true), Some(c)) if c.eq_ignore_ascii_case(own) => &[],
        (Some(true), _) => &[RunOp::Write],
        _ => &[],
    }
}

/// ADR 0025: `Run\Knock` data starts Knock's app if its program has the
/// same file name as `own`'s (the running exe), ignoring letter case, from
/// any folder, quoted or not, with or without arguments, since an older
/// copy of Knock may have written it.
fn starts_knock(data: &str, own: &str) -> bool {
    let name = program_name(own);
    !name.is_empty() && program_name(data).eq_ignore_ascii_case(name)
}

/// The file name of the program a command starts. A quoted program ends at
/// the next quote; an unquoted one at the first `.exe` that ends a word.
fn program_name(command: &str) -> &str {
    let d = command.trim_start();
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
    program.rsplit(['\\', '/']).next().unwrap_or("")
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
    /// `set_launch_at_sign_in`, `apply_sign_in_ops` and
    /// `restore_launch_at_sign_in` fail while set.
    pub sign_in_fails: std::sync::Mutex<bool>,
    /// The fake `Run\Knock` and `StartupApproved\Run\Knock` values.
    pub run: std::sync::Mutex<Option<String>>,
    pub approved: std::sync::Mutex<Option<Vec<u8>>>,
    /// What `idle_ms` returns.
    pub idle: std::sync::Mutex<Option<u64>>,
}

/// The fake's own `Run\Knock` value: the installed app (`mainBinaryName`
/// unset, so the exe keeps its Cargo name).
#[cfg(test)]
pub const FAKE_EXE: &str = r#""C:\Users\A B\AppData\Local\Knock\knock-desktop.exe""#;

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
    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<SignInChange> {
        self.sign_in.lock().unwrap().push(on);
        let change = sign_in_change(on, self.run.lock().unwrap().as_deref(), FAKE_EXE);
        self.apply_sign_in_ops(change.now)?;
        Ok(change)
    }
    fn apply_sign_in_ops(&self, ops: &[RunOp]) -> std::io::Result<()> {
        if *self.sign_in_fails.lock().unwrap() {
            return Err(std::io::Error::other("refused"));
        }
        let mut run = self.run.lock().unwrap();
        for op in ops {
            match op {
                RunOp::Write => *run = Some(FAKE_EXE.into()),
                RunOp::Delete => *run = None,
                RunOp::ClearApproved => *self.approved.lock().unwrap() = None,
            }
        }
        Ok(())
    }
    fn restore_launch_at_sign_in(&self, setting: Option<bool>) -> std::io::Result<bool> {
        if *self.sign_in_fails.lock().unwrap() {
            return Err(std::io::Error::other("refused"));
        }
        let mut run = self.run.lock().unwrap();
        let write = !start_ops(setting, run.as_deref(), FAKE_EXE).is_empty();
        if write {
            *run = Some(FAKE_EXE.into());
        }
        Ok(write)
    }
    fn idle_ms(&self) -> Option<u64> {
        *self.idle.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_value_is_the_quoted_exe_path() {
        let exe = std::path::Path::new(r"C:\Users\A B\AppData\Local\Knock\knock-desktop.exe");
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
        *fake.run.lock().unwrap() = Some(r#""C:\Other\NotKnock.exe" --x"#.into());
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
        // As the pipeline does when the save succeeds.
        let off = |fake: &FakePlatform| {
            let change = fake.set_launch_at_sign_in(false).unwrap();
            fake.apply_sign_in_ops(change.saved).unwrap();
            change
        };
        let other = r#""C:\Other\NotKnock.exe""#;
        *fake.run.lock().unwrap() = Some(other.into());
        *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
        assert_eq!(off(&fake), SignInChange::default(), "nothing to undo");
        assert_eq!(fake.run.lock().unwrap().as_deref(), Some(other), "not ours");
        assert_eq!(
            fake.approved.lock().unwrap().as_deref(),
            Some(&DISABLED[..])
        );
        // Ours, in any letter case: both values go.
        *fake.run.lock().unwrap() = Some(FAKE_EXE.to_uppercase());
        off(&fake);
        assert_eq!(*fake.run.lock().unwrap(), None);
        assert_eq!(*fake.approved.lock().unwrap(), None);
        // Nothing there: nothing to do.
        off(&fake);
        assert_eq!(*fake.run.lock().unwrap(), None);
        assert_eq!(
            sign_in_change(false, None, FAKE_EXE),
            SignInChange::default()
        );
    }

    /// The failure case of the pre-release review: on, disabled in Task
    /// Manager, turned off in Knock, and the save fails.
    #[test]
    fn an_undone_turn_off_writes_the_value_back_and_keeps_the_disabled_mark() {
        let fake = FakePlatform::default();
        let old = r#""C:\Old\Knock\knock-desktop.exe" --x"#;
        for current in [FAKE_EXE, old] {
            *fake.run.lock().unwrap() = Some(current.into());
            *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
            let change = fake.set_launch_at_sign_in(false).unwrap();
            assert_eq!(change.undo, [RunOp::Write], "{current}");
            assert!(!change.undo.contains(&RunOp::ClearApproved));
            // Before the save: the value is gone, the mark is not touched.
            assert_eq!(*fake.run.lock().unwrap(), None);
            let approved = fake.approved.lock().unwrap().clone();
            assert_eq!(approved.as_deref(), Some(&DISABLED[..]));
            // The save failed: Knock's own value is back (as ADR 0027 writes
            // it at start), and Task Manager still shows Knock disabled, so
            // Knock doesn't start at sign-in.
            fake.apply_sign_in_ops(change.undo).unwrap();
            assert_eq!(fake.run.lock().unwrap().as_deref(), Some(FAKE_EXE));
            let approved = fake.approved.lock().unwrap().clone();
            assert_eq!(approved.as_deref(), Some(&DISABLED[..]), "{current}");
        }
        // An undone turn-on deletes the value again, as before.
        *fake.run.lock().unwrap() = None;
        *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
        let change = fake.set_launch_at_sign_in(true).unwrap();
        assert_eq!(change.saved, []);
        assert_eq!(change.undo, [RunOp::Delete, RunOp::ClearApproved]);
        fake.apply_sign_in_ops(change.undo).unwrap();
        assert_eq!(*fake.run.lock().unwrap(), None);
        assert_eq!(*fake.approved.lock().unwrap(), None);
    }

    #[test]
    fn any_command_starting_the_apps_own_exe_name_is_knocks() {
        let delete = [RunOp::Delete, RunOp::ClearApproved];
        // The rule follows the running exe's name, whatever it is.
        for (own, name) in [
            (FAKE_EXE, "knock-desktop.exe"),
            (r#""D:\Apps\Knock\Knock.exe""#, "Knock.exe"),
        ] {
            // The whole turn-off, before and after the save.
            let off = |data: &str| {
                let c = sign_in_change(false, Some(data), own);
                [c.now, c.saved].concat()
            };
            let (upper, stem) = (name.to_uppercase(), name.trim_end_matches(".exe"));
            for ours in [
                own.to_string(),
                own.trim_matches('"').to_string(),
                format!(r#""C:\Old\{name}""#),
                format!(r"C:\Old\{name}"),
                format!(r#""C:\Program Files\Knock\{name}" --minimized"#),
                format!(r"C:\Program Files\Knock\{name} --minimized"),
                format!("C:\\Old\\{name}\t-x"),
                format!(r#"  "C:\OLD\{upper}""#),
                format!("c:/old/{}.Exe", stem.to_lowercase()),
                name.to_string(),
                format!(r#""C:\Old\{name}"#),
                // REG_EXPAND_SZ data, read unexpanded: only the name counts.
                format!(r#""%LOCALAPPDATA%\Knock\{name}""#),
            ] {
                assert_eq!(off(&ours), delete, "{own}: {ours}");
            }
            for theirs in [
                String::new(),
                "   ".into(),
                r#""C:\Old\NotKnock.exe""#.into(),
                format!(r"C:\Old\Not{name} --knock"),
                format!(r"C:\Old\{name}.bak"),
                format!(r#""C:\Old\{name}.bak""#),
                format!(r#""C:\Old\Other.exe" C:\Old\{name}"#),
                format!(r"C:\Tools\Other.exe C:\Old\{name}"),
                format!(r"C:\{name}\Other.exe"),
                format!(r"C:\Old\{stem}"),
            ] {
                assert!(off(&theirs).is_empty(), "{own}: {theirs}");
            }
        }
        // The app is knock-desktop.exe; the CLI, knock.exe, is not Knock's app.
        for cli in [
            r"C:\src\knock\target\release\knock.exe",
            r#""C:\Tools\knock.exe" sim demo"#,
            "knock.exe",
        ] {
            let c = sign_in_change(false, Some(cli), FAKE_EXE);
            assert_eq!(c, SignInChange::default(), "{cli}");
        }
        // Turning on never looks at the value.
        for current in [None, Some(""), Some("anything")] {
            let on = sign_in_change(true, current, FAKE_EXE);
            assert_eq!(on.now, [RunOp::Write, RunOp::ClearApproved]);
        }
    }

    #[test]
    fn at_start_a_missing_or_stale_entry_is_rewritten_while_on() {
        let write = [RunOp::Write];
        let on = |current: Option<&str>| start_ops(Some(true), current, FAKE_EXE);
        assert_eq!(on(None), write, "missing");
        assert_eq!(on(Some(FAKE_EXE)), [], "own");
        assert_eq!(on(Some(&FAKE_EXE.to_uppercase())), [], "own, other case");
        let old = r#""C:\Old\Knock\knock-desktop.exe""#;
        assert_eq!(on(Some(old)), write, "old path");
        assert_eq!(
            on(Some(r#""C:\Other\NotKnock.exe""#)),
            write,
            "other program"
        );
        assert_eq!(on(Some(FAKE_EXE.trim_matches('"'))), write, "unquoted");
        assert_eq!(on(Some(&format!("{FAKE_EXE} --x"))), write, "arguments");
        for setting in [Some(false), None] {
            for current in [None, Some(FAKE_EXE), Some(old)] {
                assert_eq!(start_ops(setting, current, FAKE_EXE), [], "{setting:?}");
            }
        }
        // Through the fake: the Task Manager mark is left as it was.
        let fake = FakePlatform::default();
        *fake.approved.lock().unwrap() = Some(DISABLED.to_vec());
        assert!(fake.restore_launch_at_sign_in(Some(true)).unwrap());
        assert_eq!(fake.run.lock().unwrap().as_deref(), Some(FAKE_EXE));
        assert!(!fake.restore_launch_at_sign_in(Some(true)).unwrap());
        let approved = fake.approved.lock().unwrap().clone();
        assert_eq!(approved.as_deref(), Some(&DISABLED[..]));
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

    /// Values as `GetTickCount64` and `LASTINPUTINFO.dwTime` give them.
    #[test]
    fn idle_time_survives_the_32_bit_tick_wrap() {
        let min = 60_000u64;
        // 3 days after boot, last input 10 minutes ago: no wrap yet.
        let now = 3 * 86_400_000;
        assert_eq!(idle_from_ticks(now, (now - 10 * min) as u32), 10 * min);
        assert_eq!(idle_from_ticks(now, now as u32), 0);
        // 60 days after boot: dwTime has wrapped once, `now` (64-bit) not.
        let now = 60 * 86_400_000u64;
        assert_eq!(idle_from_ticks(now, (now - 15 * min) as u32), 15 * min);
        // The wrap falls between the last input and now.
        let wrap = 1u64 << 32;
        assert_eq!(idle_from_ticks(wrap + 256, u32::MAX - 255), 512);
        assert_eq!(idle_from_ticks(wrap, u32::MAX), 1);
        assert_eq!(
            idle_from_ticks(2 * wrap + 10 * min, (wrap - min) as u32),
            11 * min
        );
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
