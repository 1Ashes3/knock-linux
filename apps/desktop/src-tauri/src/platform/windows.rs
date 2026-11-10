//! Windows implementation of [`Platform`]. All Win32 calls live here.

use super::{Display, Monitor, Platform, RunOp, SignInChange, StripLayer, Taskbar, monitor_id};
use crate::jump::{self, Proc, ProcessId, Target, TcpRow, Win};
use crate::strip::Rect;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA,
    ERROR_UNSUPPORTED_TYPE, FILETIME, HANDLE, HWND, LPARAM, LRESULT, NO_ERROR, POINT, RECT,
    STILL_ACTIVE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITORINFOEXW,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, TCP_TABLE_OWNER_PID_CONNECTIONS,
};
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA, DefSubclassProc, SHAppBarMessage,
    SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EDD_GET_DEVICE_INTERFACE_NAME, EnumWindows, FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO,
    FlashWindowEx, GA_ROOTOWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetCursorPos,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, HWND_BOTTOM,
    HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible, MONITORINFOF_PRIMARY,
    RegisterWindowMessageW, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    ShowWindowAsync, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::{BOOL, PCWSTR, w};

pub struct WindowsPlatform {
    // HWND is !Send; the raw value is safe to use from any thread for the
    // style and position calls below.
    strip: isize,
    /// [`StripLayer`] as 0 top, 1 desktop, 2 hidden (ADR 0037).
    layer: std::sync::atomic::AtomicU8,
    /// `show_strip` ran: from then on a layer change may show the window.
    shown: std::sync::atomic::AtomicBool,
}

impl WindowsPlatform {
    pub fn new(strip: &tauri::WebviewWindow) -> tauri::Result<Self> {
        Ok(Self {
            strip: strip.hwnd()?.0 as isize,
            layer: std::sync::atomic::AtomicU8::new(0),
            shown: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn hwnd(&self) -> HWND {
        HWND(self.strip as _)
    }

    fn layer(&self) -> StripLayer {
        match self.layer.load(std::sync::atomic::Ordering::Relaxed) {
            1 => StripLayer::Desktop,
            2 => StripLayer::Hidden,
            _ => StripLayer::Top,
        }
    }
}

/// Fills `buf` from the system's cryptographic RNG (used for the §8.1 token
/// and port). Not on [`Platform`] because the server needs it without a window.
pub fn fill_random(buf: &mut [u8]) -> std::io::Result<()> {
    // SAFETY: valid mutable buffer; no algorithm handle with the system RNG flag.
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok()?;
    Ok(())
}

/// Minutes since local midnight, for quiet hours (§8.6). Not on [`Platform`]
/// because the pipeline needs it without a window.
pub fn local_minute() -> u32 {
    // SAFETY: no arguments; returns a plain struct.
    let t = unsafe { GetLocalTime() };
    u32::from(t.wHour) * 60 + u32::from(t.wMinute)
}

/// How many hard links (names) the open file has. Not on [`Platform`]
/// because hook setup needs it without a window.
pub fn link_count(file: &std::fs::File) -> std::io::Result<u32> {
    use std::os::windows::io::AsRawHandle;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: a valid open handle and out-param; the File keeps the handle open.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }?;
    Ok(info.nNumberOfLinks)
}

/// §9.7 launch at sign-in: the per-user Run key, and the value Task
/// Manager's Startup page keeps for it. Never HKLM.
const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APPROVED: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const RUN_NAME: PCWSTR = w!("Knock");

/// `HKCU\...\Run\Knock`'s data, or `None` if there is no such value or it
/// isn't a string (then it can't be Knock's). An expandable string
/// (REG_EXPAND_SZ) comes back as written, `%VAR%`s unexpanded (ADR 0025).
/// Without RRF_NOEXPAND, RegGetValueW expands it and reports REG_SZ; with
/// it, only RRF_RT_REG_EXPAND_SZ admits it (both seen on Windows 11 25H2).
fn get_run_value() -> std::io::Result<Option<String>> {
    let mut buf = vec![0u16; 260];
    loop {
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` has `size` writable bytes; constant NUL-terminated names.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN,
                RUN_NAME,
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        match r {
            ERROR_FILE_NOT_FOUND => return Ok(None),
            ERROR_UNSUPPORTED_TYPE => {
                tracing::info!("sign-in entry is not a string");
                return Ok(None);
            }
            ERROR_MORE_DATA => buf.resize((size as usize).div_ceil(2) + 1, 0),
            _ => {
                r.ok()?;
                return Ok(Some(utf16(&buf)));
            }
        }
    }
}

/// Applies the `now` part of [`super::sign_in_change`]: writes `Run\Knock`
/// with the quoted exe path, or deletes it only if it starts Knock, and
/// returns the whole change. Turning on doesn't read the current value: it
/// overwrites whatever is there.
fn set_run_value(on: bool) -> std::io::Result<SignInChange> {
    let own = super::run_value(&std::env::current_exe()?);
    let current = if on { None } else { get_run_value()? };
    let change = super::sign_in_change(on, current.as_deref(), &own);
    if !on && change.now.is_empty() && current.is_some() {
        tracing::info!("sign-in entry is not Knock's own; left as it is");
    }
    apply_run_ops(change.now, &own)?;
    Ok(change)
}

/// Writes `Run\Knock` with the quoted exe path, deletes it, or clears
/// `StartupApproved\Run\Knock`. A value that isn't there is fine.
fn apply_run_ops(ops: &[RunOp], own: &str) -> std::io::Result<()> {
    for op in ops {
        let r = match op {
            RunOp::Write => {
                let v: Vec<u16> = own.encode_utf16().chain([0]).collect();
                // SAFETY: `v` is a NUL-terminated UTF-16 string of `v.len()` units.
                unsafe {
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        RUN,
                        RUN_NAME,
                        REG_SZ.0,
                        Some(v.as_ptr().cast()),
                        (v.len() * 2) as u32,
                    )
                }
            }
            // SAFETY: constant NUL-terminated key and value names.
            RunOp::Delete => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, RUN_NAME) },
            // SAFETY: as above.
            RunOp::ClearApproved => unsafe {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED, RUN_NAME)
            },
        };
        if r != ERROR_FILE_NOT_FOUND {
            r.ok()?;
        }
    }
    Ok(())
}

/// [`Platform::set_launch_at_sign_in`] on the real entry, with the
/// debug-build guard. When the guard stops it, nothing changed, so there is
/// nothing to finish or undo.
fn launch_at_sign_in(on: bool) -> std::io::Result<SignInChange> {
    let test_var = std::env::var_os("KNOCK_AUTOSTART_TEST");
    if !super::sign_in_allowed(cfg!(debug_assertions), test_var.as_deref()) {
        tracing::info!(on, "debug build: sign-in entry not changed");
        return Ok(SignInChange::default());
    }
    set_run_value(on)
}

/// [`Platform::apply_sign_in_ops`] on the real entry, with the same guard.
fn apply_sign_in(ops: &[RunOp]) -> std::io::Result<()> {
    let test_var = std::env::var_os("KNOCK_AUTOSTART_TEST");
    if ops.is_empty() || !super::sign_in_allowed(cfg!(debug_assertions), test_var.as_deref()) {
        return Ok(());
    }
    apply_run_ops(ops, &super::run_value(&std::env::current_exe()?))
}

/// [`Platform::restore_launch_at_sign_in`]: [`super::start_ops`] on the
/// real entry, with the same debug-build guard.
fn restore_sign_in(setting: Option<bool>) -> std::io::Result<bool> {
    let test_var = std::env::var_os("KNOCK_AUTOSTART_TEST");
    if !super::sign_in_allowed(cfg!(debug_assertions), test_var.as_deref()) {
        tracing::info!("debug build: sign-in entry not checked");
        return Ok(false);
    }
    let own = super::run_value(&std::env::current_exe()?);
    let ops = super::start_ops(setting, get_run_value()?.as_deref(), &own);
    apply_run_ops(ops, &own)?;
    Ok(!ops.is_empty())
}

/// AF_INET: the server listens on IPv4 loopback only.
const AF_INET: u32 = 2;

/// The current IPv4 TCP connection table with owner PIDs.
fn tcp_table() -> Option<Vec<TcpRow>> {
    // u32s keep the buffer aligned for MIB_TCPTABLE_OWNER_PID.
    let mut buf: Vec<u32> = Vec::new();
    for _ in 0..4 {
        let mut size = (buf.len() * 4) as u32;
        let ptr = (!buf.is_empty()).then(|| buf.as_mut_ptr().cast());
        // SAFETY: `ptr` is None or points to `size` writable, aligned bytes.
        let ret = unsafe {
            GetExtendedTcpTable(
                ptr,
                &mut size,
                false,
                AF_INET,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            )
        };
        if ret == NO_ERROR.0 && !buf.is_empty() {
            let n = (buf[0] as usize).min((buf.len() - 1) / 6);
            return Some(buf[1..=n * 6].as_chunks::<6>().0.to_vec());
        }
        if ret != ERROR_INSUFFICIENT_BUFFER.0 {
            return None;
        }
        // Room for a few more connections opened in between.
        buf.resize((size as usize).div_ceil(4) + 6 * 16, 0);
    }
    None
}

/// Creation time of `pid` while it is still running, or `None` if it has
/// exited or can't be opened. A process that exited but whose handle someone
/// still holds can be opened; its exit code tells it apart.
fn process_start(pid: u32) -> Option<u64> {
    // SAFETY: the handle is checked by `?` and closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut code = 0u32;
        let running = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let [mut c, mut e, mut k, mut u] = [FILETIME::default(); 4];
        let ok = running && GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
    }
}

/// §9.7 idle time: the system's last-input time (this Windows session, any
/// app), read first, then the tick count, so `now` is never before it.
/// Reads only; nothing here sends or simulates input (§9.6).
fn idle_ms() -> Option<u64> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: valid, initialized out-param with its size set.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    // SAFETY: no arguments.
    let now = unsafe { GetTickCount64() };
    Some(super::idle_from_ticks(now, info.dwTime))
}

/// Every process: PID, parent, start time and exe file name.
fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is checked and closed; the entry's dwSize is set.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let len = e
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(e.szExeFile.len());
            out.push(Proc {
                pid: e.th32ProcessID,
                parent: e.th32ParentProcessID,
                start: process_start(e.th32ProcessID),
                exe: String::from_utf16_lossy(&e.szExeFile[..len]),
            });
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

struct EnumState<'a> {
    pids: &'a [u32],
    project: String,
    out: Vec<Win>,
}

/// Top-level windows owned by any of `pids`, each described by its root owner.
fn windows_of(pids: &[u32], project: &str) -> Vec<Win> {
    let mut st = EnumState {
        pids,
        project: project.to_lowercase(),
        out: Vec::new(),
    };
    // SAFETY: `st` outlives the synchronous enumeration.
    let _ = unsafe { EnumWindows(Some(enum_window), LPARAM(&raw mut st as isize)) };
    st.out
}

unsafe extern "system" fn enum_window(h: HWND, l: LPARAM) -> BOOL {
    // SAFETY: `l` is the `EnumState` passed by `windows_of`.
    let st = unsafe { &mut *(l.0 as *mut EnumState) };
    let mut pid = 0;
    // SAFETY: plain queries on a handle from EnumWindows. For another
    // process's window, GetWindowTextW reads the cached caption and sends no
    // message, so a hung app can't block this.
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
        if !st.pids.contains(&pid) {
            return true.into();
        }
        let root = GetAncestor(h, GA_ROOTOWNER);
        let root = if root.is_invalid() { h } else { root };
        let mut root_pid = 0;
        GetWindowThreadProcessId(root, Some(&mut root_pid));
        let mut class = [0u16; 256];
        let n = GetClassNameW(root, &mut class).max(0) as usize;
        let mut title = [0u16; 512];
        let t = GetWindowTextW(root, &mut title).max(0) as usize;
        // The title is compared and dropped, never kept or logged.
        let has_project = !st.project.is_empty()
            && String::from_utf16_lossy(&title[..t])
                .to_lowercase()
                .contains(&st.project);
        st.out.push(Win {
            pid,
            root: root.0 as isize,
            root_pid,
            root_visible: IsWindowVisible(root).as_bool(),
            root_class: String::from_utf16_lossy(&class[..n]),
            root_has_project: has_project,
        });
    }
    true.into()
}

fn utf16(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

unsafe extern "system" fn each_monitor(h: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
    // SAFETY: `l` is the Vec passed by `monitors`.
    unsafe { (*(l.0 as *mut Vec<HMONITOR>)).push(h) };
    true.into()
}

/// Every display: its device interface path (stable), the name Windows
/// gives it, primary flag, resolution, work area and scale. Primary first.
fn screens() -> Vec<(Monitor, Display)> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: `handles` outlives the synchronous enumeration.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(each_monitor),
            LPARAM(&raw mut handles as isize),
        )
    };
    let mut out: Vec<(Monitor, Display)> = (handles.iter().enumerate())
        .filter_map(|(i, &h)| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            let mut dd = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            let (mut dpi, mut dpi_y) = (96, 96);
            // SAFETY: valid out-params with their sizes set; szDevice is
            // NUL-terminated by GetMonitorInfoW.
            let found = unsafe {
                if !GetMonitorInfoW(h, (&raw mut info).cast()).as_bool() {
                    return None;
                }
                if GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y).is_err() {
                    dpi = 96;
                }
                let device = PCWSTR(info.szDevice.as_ptr());
                EnumDisplayDevicesW(device, 0, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME).as_bool()
            };
            let r = info.monitorInfo.rcMonitor;
            // Without an interface path, the GDI name (\\.\DISPLAY1) is the
            // best id there is; it can change when displays are re-plugged.
            let path = Some(utf16(&dd.DeviceID))
                .filter(|p| found && !p.is_empty())
                .unwrap_or_else(|| utf16(&info.szDevice));
            let name = Some(utf16(&dd.DeviceString)).filter(|n| found && !n.is_empty());
            let (id, primary) = (
                monitor_id(&path),
                info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            );
            let m = Monitor {
                id: id.clone(),
                name: name.unwrap_or_else(|| format!("Display {}", i + 1)),
                primary,
                width: r.right - r.left,
                height: r.bottom - r.top,
            };
            let d = Display {
                id,
                primary,
                monitor: rect(r),
                work_area: rect(info.monitorInfo.rcWork),
                scale: f64::from(dpi) / 96.0,
            };
            Some((m, d))
        })
        .collect();
    out.sort_by_key(|(m, _)| !m.primary);
    out
}

fn monitors() -> Vec<Monitor> {
    screens().into_iter().map(|(m, _)| m).collect()
}

/// "TaskbarCreated": broadcast to every top-level window when Explorer
/// (re)creates the taskbar.
static TASKBAR_CREATED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// The strip's subclass: flags display, DPI, work-area (any
/// WM_SETTINGCHANGE, including SPI_SETWORKAREA and "TraySettings") and
/// taskbar-recreated messages, then hands every message on unchanged.
unsafe extern "system" fn watch(
    h: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    flag: usize,
) -> LRESULT {
    let taskbar = TASKBAR_CREATED.get().is_some_and(|&m| m != 0 && m == msg);
    if matches!(msg, WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE) || taskbar {
        // SAFETY: `flag` is the AtomicBool leaked by `watch_displays`.
        let flag = unsafe { &*(flag as *const std::sync::atomic::AtomicBool) };
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // SAFETY: the arguments as received.
    unsafe { DefSubclassProc(h, msg, w, l) }
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
    fn displays(&self) -> Vec<Display> {
        screens().into_iter().map(|(_, d)| d).collect()
    }

    fn taskbar(&self) -> Option<Taskbar> {
        let mut bar = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        // SAFETY: valid, initialized APPBARDATA.
        unsafe {
            (SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) != 0).then(|| Taskbar {
                rect: rect(bar.rc),
                edge: bar.uEdge,
                auto_hide: SHAppBarMessage(ABM_GETSTATE, &mut bar) & ABS_AUTOHIDE as usize != 0,
            })
        }
    }

    fn strip_rect(&self) -> Option<Rect> {
        let mut r = RECT::default();
        // SAFETY: valid window handle owned by this process; valid out-param.
        unsafe { GetWindowRect(self.hwnd(), &mut r) }.ok()?;
        Some(rect(r))
    }

    fn watch_displays(&self, changed: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        // SAFETY: constant NUL-terminated name.
        TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
        // Leaked on purpose: the strip lives as long as the app.
        let flag = std::sync::Arc::into_raw(changed) as usize;
        // SAFETY: called on the strip's own (main) thread, as
        // SetWindowSubclass requires; `flag` stays valid forever.
        if !unsafe { SetWindowSubclass(self.hwnd(), Some(watch), 1, flag) }.as_bool() {
            tracing::warn!("display changes not watched; polling only");
        }
    }

    fn place_strip(&self, r: Rect) -> std::io::Result<()> {
        self.set_click_through(true);
        // Desktop and hidden: at the bottom of the z-order, which also drops
        // the topmost style (ADR 0037).
        let after = match self.layer() {
            StripLayer::Top => HWND_TOPMOST,
            _ => HWND_BOTTOM,
        };
        // SAFETY: valid window handle owned by this process.
        unsafe {
            SetWindowPos(
                self.hwnd(),
                Some(after),
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

    fn show_strip(&self) {
        self.shown.store(true, std::sync::atomic::Ordering::Relaxed);
        if self.layer() == StripLayer::Hidden {
            return;
        }
        // Not tao's `set_visible`: it rewrites the styles. tao keeps thinking
        // the strip is hidden, so any tao setter on it would hide it again
        // (tao `apply_diff`); ADR 0001 already rules those out.
        // SAFETY: valid window handle owned by this process.
        unsafe {
            let _ = ShowWindow(self.hwnd(), SW_SHOWNOACTIVATE);
        }
        self.set_click_through(true);
    }

    fn set_strip_layer(&self, layer: StripLayer) {
        let n = match layer {
            StripLayer::Top => 0,
            StripLayer::Desktop => 1,
            StripLayer::Hidden => 2,
        };
        self.layer.store(n, std::sync::atomic::Ordering::Relaxed);
        let shown = self.shown.load(std::sync::atomic::Ordering::Relaxed);
        // SAFETY: valid window handle owned by this process. Async, so the
        // caller (any thread) never waits on the strip's thread.
        unsafe {
            if layer == StripLayer::Hidden {
                let _ = ShowWindowAsync(self.hwnd(), SW_HIDE);
                return;
            }
        }
        if let Some(r) = self.strip_rect()
            && let Err(e) = self.place_strip(r)
        {
            tracing::warn!(kind = ?e.kind(), "strip layer not changed");
        }
        if shown {
            // SAFETY: as above.
            unsafe {
                let _ = ShowWindowAsync(self.hwnd(), SW_SHOWNOACTIVATE);
            }
        }
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

    fn connection_owner(&self, client_port: u16, server_port: u16) -> Option<ProcessId> {
        let pid = jump::tcp_owner(&tcp_table()?, client_port, server_port)?;
        Some(ProcessId {
            pid,
            start: process_start(pid)?,
        })
    }

    fn find_window(&self, process: ProcessId, project: &str) -> Option<Target> {
        let procs = processes();
        let chain = jump::chain(&procs, process);
        let pids: Vec<u32> = chain.iter().map(|p| p.pid).collect();
        jump::pick(&procs, &chain, &windows_of(&pids, project))
    }

    fn focus_window(&self, window: isize) -> bool {
        let h = HWND(window as _);
        // SAFETY: plain calls on a window handle; a stale handle just fails.
        // None of them waits on the target's process (§9.6, ADR 0013).
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return false;
            }
            let minimized = IsIconic(h).as_bool();
            let front = SetForegroundWindow(h).as_bool();
            if front && minimized {
                // Async, so a hung target can't block the caller.
                let _ = ShowWindowAsync(h, SW_RESTORE);
            }
            if !front {
                let info = FLASHWINFO {
                    cbSize: size_of::<FLASHWINFO>() as u32,
                    hwnd: h,
                    dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                    uCount: 0,
                    dwTimeout: 0,
                };
                let _ = FlashWindowEx(&info);
            }
            front
        }
    }

    fn process_start(&self, pid: u32) -> Option<u64> {
        process_start(pid)
    }

    fn agent_process(&self, helper: ProcessId) -> Option<ProcessId> {
        jump::agent_of(&processes(), helper)
    }

    fn monitors(&self) -> Vec<Monitor> {
        monitors()
    }

    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<SignInChange> {
        launch_at_sign_in(on)
    }

    fn apply_sign_in_ops(&self, ops: &[RunOp]) -> std::io::Result<()> {
        apply_sign_in(ops)
    }

    fn restore_launch_at_sign_in(&self, setting: Option<bool>) -> std::io::Result<bool> {
        restore_sign_in(setting)
    }

    fn idle_ms(&self) -> Option<u64> {
        idle_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    // Real OS calls: at least one display, exactly one primary and first,
    // and the same ids on a second read.
    #[test]
    fn monitors_of_this_machine() {
        let a = monitors();
        assert!(!a.is_empty());
        assert!(a[0].primary);
        assert_eq!(a.iter().filter(|m| m.primary).count(), 1);
        assert!(
            a.iter()
                .all(|m| m.width > 0 && m.height > 0 && !m.name.is_empty())
        );
        assert_eq!(a, monitors());
    }

    /// `reg query` output for `HKCU\<key>`, value `Knock`; `None` if absent.
    fn reg_knock(key: &str) -> Option<String> {
        let out = std::process::Command::new("reg.exe")
            .args(["query", &format!(r"HKCU\{key}"), "/v", "Knock"])
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    // Real OS calls on the real per-user Run key, so only on request:
    // `KNOCK_AUTOSTART_TEST=1 cargo test -p knock-desktop -- --ignored run_key`.
    // Removes what it adds.
    #[test]
    #[ignore = "writes the real HKCU Run key; set KNOCK_AUTOSTART_TEST=1"]
    fn run_key_holds_the_quoted_exe_and_is_removed() {
        assert_eq!(std::env::var("KNOCK_AUTOSTART_TEST").as_deref(), Ok("1"));
        let run = r"Software\Microsoft\Windows\CurrentVersion\Run";
        let approved = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
        assert_eq!(reg_knock(run), None, "an entry was already there");
        // Removes both values even if an assertion below fails.
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for key in ["Run", r"Explorer\StartupApproved\Run"] {
                    let key = format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\{key}");
                    let _ = std::process::Command::new("reg.exe")
                        .args(["delete", &key, "/v", "Knock", "/f"])
                        .output();
                }
            }
        }
        let _cleanup = Cleanup;
        let p = WindowsPlatform {
            strip: 0,
            layer: std::sync::atomic::AtomicU8::new(0),
            shown: std::sync::atomic::AtomicBool::new(false),
        };
        // A turn-off as the pipeline runs it when the save succeeds.
        let off = || {
            let change = p.set_launch_at_sign_in(false).unwrap();
            p.apply_sign_in_ops(change.saved).unwrap();
        };
        p.set_launch_at_sign_in(true).unwrap();
        let exe = std::env::current_exe().unwrap();
        let got = reg_knock(run).unwrap();
        eprintln!("{}", got.trim());
        let want = format!("REG_SZ    \"{}\"", exe.display());
        assert!(got.contains(&want), "{got}");
        off();
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));
        off();

        let reg = |args: &[&str]| {
            let ok = std::process::Command::new("reg.exe")
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "{args:?}");
        };
        let (hk_run, hk_approved) = (format!(r"HKCU\{run}"), format!(r"HKCU\{approved}"));
        // Disabled in Task Manager (03 00 00 00 + time): enabling clears it.
        let disabled = ["add", &hk_approved, "/v", "Knock", "/t", "REG_BINARY"];
        reg(&[&disabled[..], &["/d", "030000000000000000000000", "/f"]].concat());
        p.set_launch_at_sign_in(true).unwrap();
        assert!(reg_knock(run).unwrap().contains(&want));
        assert_eq!(reg_knock(approved), None, "Task Manager shows it enabled");
        // Disabled again in Task Manager, then a turn-off whose save fails:
        // the undo writes the value back and the mark stays.
        reg(&[&disabled[..], &["/d", "030000000000000000000000", "/f"]].concat());
        let change = p.set_launch_at_sign_in(false).unwrap();
        assert_eq!(reg_knock(run), None);
        assert!(reg_knock(approved).is_some(), "not cleared before the save");
        p.apply_sign_in_ops(change.undo).unwrap();
        assert!(reg_knock(run).unwrap().contains(&want));
        assert!(reg_knock(approved).is_some(), "the user's mark stays");
        off();
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));
        // An older copy of Knock's value, in another folder, plain or
        // expandable (`%VAR%`s left as written): off removes it. Knock's app
        // is whatever this exe is named (ADR 0025).
        let name = exe.file_name().unwrap().to_str().unwrap();
        let old = format!(r#""C:\Old\{name}""#);
        let expand = format!(r#""%LOCALAPPDATA%\Old\{name}" --x"#);
        for (kind, data) in [("REG_SZ", &old), ("REG_EXPAND_SZ", &expand)] {
            reg(&["add", &hk_run, "/v", "Knock", "/t", kind, "/d", data, "/f"]);
            reg(&[&disabled[..], &["/d", "030000000000000000000000", "/f"]].concat());
            let before = reg_knock(run).unwrap();
            eprintln!("{}", before.trim());
            assert!(before.contains(&format!("{kind}    {data}")), "{before}");
            off();
            assert_eq!(
                (reg_knock(run), reg_knock(approved)),
                (None, None),
                "{kind}"
            );
        }
        // An expandable value that starts another program survives off.
        let other = r"%LOCALAPPDATA%\Other\knock.exe";
        reg(&[
            "add",
            &hk_run,
            "/v",
            "Knock",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            other,
            "/f",
        ]);
        off();
        let kept = reg_knock(run);
        reg(&["delete", &hk_run, "/v", "Knock", "/f"]);
        assert!(kept.unwrap().contains(other));
        // A value that isn't a string: off leaves it, on overwrites it.
        let dword = [
            "add",
            &hk_run,
            "/v",
            "Knock",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ];
        reg(&dword);
        off();
        let kept = reg_knock(run);
        p.set_launch_at_sign_in(true).unwrap();
        let written = reg_knock(run);
        off();
        assert!(kept.unwrap().contains("REG_DWORD"));
        assert!(written.unwrap().contains(&want));
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));
        // A value under the same name that starts another program survives off.
        let other = r#""C:\Other\NotKnock.exe""#;
        reg(&[
            "add", &hk_run, "/v", "Knock", "/t", "REG_SZ", "/d", other, "/f",
        ]);
        reg(&[&disabled[..], &["/d", "030000000000000000000000", "/f"]].concat());
        off();
        let kept = (reg_knock(run), reg_knock(approved));
        reg(&["delete", &hk_run, "/v", "Knock", "/f"]);
        reg(&["delete", &hk_approved, "/v", "Knock", "/f"]);
        assert!(kept.0.unwrap().contains(other));
        assert!(kept.1.is_some());
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));

        // ADR 0027, at start. Off or unset: nothing, even if missing.
        assert!(!p.restore_launch_at_sign_in(Some(false)).unwrap());
        assert!(!p.restore_launch_at_sign_in(None).unwrap());
        assert_eq!(reg_knock(run), None);
        // On and missing: written; a disabled mark stays.
        reg(&[&disabled[..], &["/d", "030000000000000000000000", "/f"]].concat());
        assert!(p.restore_launch_at_sign_in(Some(true)).unwrap());
        assert!(reg_knock(run).unwrap().contains(&want));
        assert!(reg_knock(approved).is_some(), "the user's mark stays");
        // Already Knock's own: left.
        assert!(!p.restore_launch_at_sign_in(Some(true)).unwrap());
        // Another program's or an old path: rewritten.
        for data in [other, r#""C:\Old\knock-desktop.exe""#] {
            reg(&[
                "add", &hk_run, "/v", "Knock", "/t", "REG_SZ", "/d", data, "/f",
            ]);
            assert!(p.restore_launch_at_sign_in(Some(true)).unwrap());
            assert!(reg_knock(run).unwrap().contains(&want), "{data}");
        }
        off();
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));
    }

    // Real OS calls (§9.7 idle time): readable, and never longer than the
    // time since boot. (Not compared across reads: the user may move the
    // mouse in between.)
    #[test]
    fn idle_time_of_this_machine() {
        let idle = idle_ms().unwrap();
        // SAFETY: no arguments.
        let up = unsafe { GetTickCount64() };
        assert!(idle <= up, "{idle} > {up}");
    }

    // Real OS calls (V-7 mechanics): this test process owns the client end of
    // its own loopback connection, and its parent chain can be walked.
    #[test]
    fn connection_owner_and_chain_of_this_process() {
        let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(server.local_addr().unwrap()).unwrap();
        let (_conn, _) = server.accept().unwrap();
        let (cport, sport) = (
            client.local_addr().unwrap().port(),
            server.local_addr().unwrap().port(),
        );
        let me = std::process::id();
        assert_eq!(
            jump::tcp_owner(&tcp_table().unwrap(), cport, sport),
            Some(me)
        );

        let start = process_start(me).unwrap();
        let procs = processes();
        let chain = jump::chain(&procs, ProcessId { pid: me, start });
        assert!(
            chain[0].exe.starts_with("knock_desktop"),
            "{}",
            chain[0].exe
        );
        assert!(chain.len() >= 2, "a test runs under cargo or a shell");
        // §9.7 liveness, real calls: this process runs, an exited child doesn't.
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/C", "exit 0"])
            .spawn()
            .unwrap();
        let held = child.id();
        child.wait().unwrap();
        // `child` still holds its handle, so the exited process can be opened.
        assert_eq!(process_start(held), None);
        drop(child);
        // A wrong start time is treated as a reused PID.
        assert!(
            jump::chain(
                &procs,
                ProcessId {
                    pid: me,
                    start: start + 1
                }
            )
            .is_empty()
        );
    }
}
