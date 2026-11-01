//! Windows implementation of [`Platform`]. All Win32 calls live here.

use super::{Display, Monitor, Platform, Taskbar, monitor_id};
use crate::jump::{self, Proc, ProcessId, Target, TcpRow, Win};
use crate::strip::Rect;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, FILETIME, HANDLE, HWND, LPARAM,
    NO_ERROR, POINT, RECT, STILL_ACTIVE,
};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
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
    HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EDD_GET_DEVICE_INTERFACE_NAME, EnumWindows, FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO,
    FlashWindowEx, GA_ROOTOWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetCursorPos,
    GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId, HWND_TOPMOST, IsIconic, IsWindow,
    IsWindowVisible, MONITORINFOF_PRIMARY, SW_RESTORE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    ShowWindowAsync, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};
use windows::core::{BOOL, PCWSTR, w};

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

/// Writes `HKCU\...\Run\Knock` with the quoted exe path, or deletes it and
/// any `StartupApproved\Run\Knock`. A value that isn't there is fine.
fn set_run_value(on: bool) -> std::io::Result<()> {
    if on {
        let exe = std::env::current_exe()?;
        let v: Vec<u16> = super::run_value(&exe).encode_utf16().chain([0]).collect();
        // SAFETY: `v` is a NUL-terminated UTF-16 string of `v.len()` units.
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN,
                RUN_NAME,
                REG_SZ.0,
                Some(v.as_ptr().cast()),
                (v.len() * 2) as u32,
            )
        };
        return r.ok().map_err(Into::into);
    }
    for key in [RUN, APPROVED] {
        // SAFETY: constant NUL-terminated key and value names.
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, RUN_NAME) };
        if r != ERROR_FILE_NOT_FOUND {
            r.ok()?;
        }
    }
    Ok(())
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
/// gives it, primary flag and resolution. Primary first.
fn monitors() -> Vec<Monitor> {
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
    let mut out: Vec<Monitor> = (handles.iter().enumerate())
        .filter_map(|(i, &h)| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            let mut dd = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            // SAFETY: valid out-params with their sizes set; szDevice is
            // NUL-terminated by GetMonitorInfoW.
            let found = unsafe {
                if !GetMonitorInfoW(h, (&raw mut info).cast()).as_bool() {
                    return None;
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
            Some(Monitor {
                id: monitor_id(&path),
                name: name.unwrap_or_else(|| format!("Display {}", i + 1)),
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                width: r.right - r.left,
                height: r.bottom - r.top,
            })
        })
        .collect();
    out.sort_by_key(|m| !m.primary);
    out
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

    fn show_strip(&self) {
        // Not tao's `set_visible`: it rewrites the styles. tao keeps thinking
        // the strip is hidden, so any tao setter on it would hide it again
        // (tao `apply_diff`); ADR 0001 already rules those out.
        // SAFETY: valid window handle owned by this process.
        unsafe {
            let _ = ShowWindow(self.hwnd(), SW_SHOWNOACTIVATE);
        }
        self.set_click_through(true);
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

    fn monitors(&self) -> Vec<Monitor> {
        monitors()
    }

    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<()> {
        let test_var = std::env::var_os("KNOCK_AUTOSTART_TEST");
        if !super::sign_in_allowed(cfg!(debug_assertions), test_var.as_deref()) {
            tracing::info!(on, "debug build: sign-in entry not changed");
            return Ok(());
        }
        set_run_value(on)
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
        let p = WindowsPlatform { strip: 0 };
        p.set_launch_at_sign_in(true).unwrap();
        let exe = std::env::current_exe().unwrap();
        let got = reg_knock(run).unwrap();
        eprintln!("{}", got.trim());
        let want = format!("REG_SZ    \"{}\"", exe.display());
        assert!(got.contains(&want), "{got}");
        p.set_launch_at_sign_in(false).unwrap();
        assert_eq!((reg_knock(run), reg_knock(approved)), (None, None));
        p.set_launch_at_sign_in(false).unwrap();
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
