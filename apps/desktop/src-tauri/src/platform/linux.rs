//! Linux implementation of [`Platform`] (ADR 0037). GTK calls run on the
//! main thread; `/proc` and D-Bus reads run on any thread.
//!
//! The strip has one of three backends, picked at start:
//! - **Layer shell** (Wayland with wlr-layer-shell: KDE, Hyprland, Sway,
//!   niri, COSMIC): a layer surface anchored bottom-right. The compositor
//!   keeps it clear of panels, so there is no taskbar to read.
//! - **X11**: an undecorated keep-above window, placed with the work area.
//! - **None** (Wayland without layer shell, such as GNOME): no strip; tray,
//!   notifications and settings still work.
//!
//! Click-through: Wayland gives no global cursor position, so instead of
//! polling the cursor (§9.3) the strip's input region is set to the UI's
//! hit regions, and everything else passes through.

use super::{Display, Monitor, Platform, RunOp, SignInChange, StripLayer, Taskbar, monitor_id};
use crate::jump::{self, Precision, Proc, ProcessId, Target};
use crate::strip::{Rect, Region};
use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use gtk_layer_shell::LayerShell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::ThreadId;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    LayerShell,
    X11,
    None,
}

thread_local! {
    /// The strip's GTK window. GTK objects live on the main thread only.
    static STRIP: RefCell<Option<gtk::ApplicationWindow>> = const { RefCell::new(None) };
}

pub struct LinuxPlatform {
    backend: Backend,
    main: ThreadId,
    /// Where the strip was last placed: a layer surface can't be asked.
    placed: Mutex<Option<Rect>>,
    /// The last hit regions, applied again after every placement.
    regions: Mutex<Vec<Region>>,
    /// Session PID → (its process chain, project), from the last
    /// `find_window`, for `focus_window`.
    jumps: Mutex<HashMap<isize, (Vec<u32>, String)>>,
    /// The layer asked for last; `show_strip` keeps a hidden strip hidden.
    layer: Mutex<StripLayer>,
    /// `show_strip` ran: from then on a layer change may show the window.
    shown: AtomicBool,
}

impl LinuxPlatform {
    /// Call on the main thread, before the strip is first shown: a layer
    /// surface must be set up before GTK realizes the window.
    pub fn new(strip: &tauri::WebviewWindow) -> tauri::Result<Self> {
        let window = strip.gtk_window()?;
        let wayland = gdk::Display::default().is_some_and(|d| d.type_().name().contains("Wayland"));
        let backend = match (wayland, gtk_layer_shell::is_supported()) {
            (true, true) => Backend::LayerShell,
            (true, false) => Backend::None,
            (false, _) => Backend::X11,
        };
        match backend {
            Backend::LayerShell => {
                LayerShell::init_layer_shell(&window);
                LayerShell::set_namespace(&window, "knock");
                LayerShell::set_layer(&window, gtk_layer_shell::Layer::Top);
                LayerShell::set_keyboard_mode(&window, gtk_layer_shell::KeyboardMode::None);
                LayerShell::set_anchor(&window, gtk_layer_shell::Edge::Bottom, true);
                LayerShell::set_anchor(&window, gtk_layer_shell::Edge::Right, true);
                // 0: kept clear of panels, and reserves no space itself.
                LayerShell::set_exclusive_zone(&window, 0);
            }
            Backend::X11 => {
                window.set_type_hint(gdk::WindowTypeHint::Notification);
                window.set_keep_above(true);
                window.set_skip_taskbar_hint(true);
                window.set_skip_pager_hint(true);
                window.set_accept_focus(false);
                window.set_focus_on_map(false);
                window.stick();
            }
            Backend::None => {
                tracing::warn!("this Wayland desktop has no layer shell; strip not shown");
            }
        }
        tracing::info!(?backend, "linux strip backend");
        // Leaving the input region is a pointer leave for the whole surface,
        // which WebKitGTK doesn't turn into `mouseleave` on the element under
        // the cursor. It also keeps the last position and replays it as a
        // fake mouse move when the page changes, hovering whatever is there.
        // So the overlay hears when the pointer really enters and leaves.
        for (entering, event) in [(false, "pointer-left"), (true, "pointer-entered")] {
            let webview = strip.clone();
            let notify = move |e: &gdk::EventCrossing| {
                if e.detail() != gdk::NotifyType::Inferior {
                    let _ = tauri::Emitter::emit_to(&webview, "strip", event, ());
                }
                glib::Propagation::Proceed
            };
            if entering {
                window.connect_enter_notify_event(move |_, e| notify(e));
            } else {
                window.connect_leave_notify_event(move |_, e| notify(e));
            }
        }
        STRIP.with(|s| *s.borrow_mut() = Some(window));
        start_idle_watch();
        Ok(Self {
            backend,
            main: std::thread::current().id(),
            placed: Mutex::new(None),
            regions: Mutex::new(Vec::new()),
            jumps: Mutex::new(HashMap::new()),
            layer: Mutex::new(StripLayer::Top),
            shown: AtomicBool::new(false),
        })
    }

    /// Runs `f` on the GTK main thread and returns its result; `None` if
    /// the main loop didn't run it within 2 s.
    fn on_main<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        if std::thread::current().id() == self.main {
            return Some(f());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        glib::MainContext::default().invoke(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }
}

fn with_strip<R>(f: impl FnOnce(&gtk::ApplicationWindow) -> R) -> Option<R> {
    STRIP.with(|s| s.borrow().as_ref().map(f))
}

/// Fills `buf` from the kernel's random pool (§8.1 token and port).
pub fn fill_random(buf: &mut [u8]) -> std::io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

/// Minutes since local midnight, for quiet hours (§8.6).
pub fn local_minute() -> u32 {
    // SAFETY: `time` with a null pointer only returns; `localtime_r` writes
    // into our zeroed `tm` and returns null on failure.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        (tm.tm_hour * 60 + tm.tm_min) as u32
    }
}

/// How many hard links (names) the open file has.
pub fn link_count(file: &std::fs::File) -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink() as u32)
}

// ---------------------------------------------------------------- displays

/// Every monitor as GDK reports it, with its physical rects. GDK gives
/// logical px; physical is logical times the monitor's scale. Primary first
/// (Wayland has no primary: the first monitor stands in).
fn screens() -> Vec<(Monitor, Display, gdk::Monitor)> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let n = display.n_monitors();
    let any_primary = (0..n).any(|i| display.monitor(i).is_some_and(|m| m.is_primary()));
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut out: Vec<_> = (0..n)
        .filter_map(|i| {
            let m = display.monitor(i)?;
            let scale = f64::from(m.scale_factor().max(1));
            let phys = |r: gdk::Rectangle| Rect {
                left: (f64::from(r.x()) * scale) as i32,
                top: (f64::from(r.y()) * scale) as i32,
                right: (f64::from(r.x() + r.width()) * scale) as i32,
                bottom: (f64::from(r.y() + r.height()) * scale) as i32,
            };
            let geometry = phys(m.geometry());
            let work = phys(m.workarea());
            let maker = m.manufacturer().map(|s| s.to_string()).unwrap_or_default();
            let model = m.model().map(|s| s.to_string()).unwrap_or_default();
            // Two identical monitors get the same key: number the second.
            let key = format!("{maker}|{model}");
            let nth = seen.entry(key.clone()).and_modify(|c| *c += 1).or_insert(0);
            let id = monitor_id(&format!("{key}|{nth}"));
            let primary = if any_primary { m.is_primary() } else { i == 0 };
            let name = format!("{maker} {model}").trim().to_string();
            let mon = Monitor {
                id: id.clone(),
                name: if name.is_empty() {
                    format!("Display {}", i + 1)
                } else {
                    name
                },
                primary,
                width: geometry.right - geometry.left,
                height: geometry.bottom - geometry.top,
            };
            let disp = Display {
                id,
                primary,
                monitor: geometry,
                work_area: work,
                scale,
            };
            Some((mon, disp, m))
        })
        .collect();
    out.sort_by_key(|(m, _, _)| !m.primary);
    out
}

/// The monitor whose physical rect holds the strip's bottom-right corner,
/// else the first.
fn monitor_for(r: Rect) -> Option<(Display, gdk::Monitor)> {
    let all = screens();
    let (x, y) = (r.right - 1, r.bottom - 1);
    let inside = |d: &Display| {
        x >= d.monitor.left && x < d.monitor.right && y >= d.monitor.top && y < d.monitor.bottom
    };
    let i = all.iter().position(|(_, d, _)| inside(d)).unwrap_or(0);
    all.into_iter().nth(i).map(|(_, d, m)| (d, m))
}

/// Applies `regions` (logical px, from the strip's top-left) as the strip's
/// input region. Empty: the strip ignores the mouse entirely.
fn apply_input_region(window: &gtk::ApplicationWindow, regions: &[Region]) {
    let Some(gdk_window) = window.window() else {
        return;
    };
    let rects: Vec<cairo::RectangleInt> = regions
        .iter()
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .map(|r| {
            let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
            let right = (r.x + r.width).ceil() as i32;
            let bottom = (r.y + r.height).ceil() as i32;
            cairo::RectangleInt::new(x, y, right - x, bottom - y)
        })
        .collect();
    let region = cairo::Region::create_rectangles(&rects);
    gdk_window.input_shape_combine_region(&region, 0, 0);
}

// ------------------------------------------------------------------- /proc

/// `/proc/<pid>/stat` → (parent PID, start time in clock ticks since boot,
/// command name). The name sits in parentheses and may hold spaces or
/// parentheses itself, so the fields are read after the last `)`.
pub fn parse_stat(stat: &str) -> Option<(u32, u64, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    // Fields from the 3rd (state) on.
    let rest: Vec<&str> = stat.get(close + 1..)?.split_whitespace().collect();
    let parent = rest.get(1)?.parse().ok()?;
    let start = rest.get(19)?.parse().ok()?;
    Some((parent, start, comm))
}

fn read_stat(pid: u32) -> Option<(u32, u64, String)> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn process_start(pid: u32) -> Option<u64> {
    read_stat(pid).map(|(_, start, _)| start)
}

/// Every process we can read: PID, parent, start time and command name.
fn processes() -> Vec<Proc> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let (parent, start, exe) = read_stat(pid)?;
            Some(Proc {
                pid,
                parent,
                start: Some(start),
                exe,
            })
        })
        .collect()
}

/// The socket inode of the IPv4 loopback connection from `client_port` to
/// `server_port`, from the text of `/proc/net/tcp`. Addresses there are
/// hex in host byte order (`0100007F` is 127.0.0.1 on little-endian), ports
/// hex in network order.
pub fn tcp_inode(table: &str, client_port: u16, server_port: u16) -> Option<u64> {
    let loopback = |a: &str| {
        u32::from_str_radix(a, 16)
            .is_ok_and(|v| std::net::Ipv4Addr::from(v.to_ne_bytes()).is_loopback())
    };
    table.lines().skip(1).find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (local, remote) = (f.get(1)?.split_once(':')?, f.get(2)?.split_once(':')?);
        let ok = loopback(local.0)
            && u16::from_str_radix(local.1, 16).ok()? == client_port
            && loopback(remote.0)
            && u16::from_str_radix(remote.1, 16).ok()? == server_port;
        ok.then(|| f.get(9)?.parse().ok()).flatten()
    })
}

/// The process holding socket `inode` open. Only our own user's processes
/// can be read, and Claude Code runs as us.
fn socket_owner(inode: u64) -> Option<u32> {
    let target = format!("socket:[{inode}]");
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        let fds = std::fs::read_dir(e.path().join("fd")).ok()?;
        fds.flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.as_os_str() == target.as_str()))
            .then_some(pid)
    })
}

/// Processes that end the walk up from a session: the session manager,
/// shells of the desktop itself, and terminal multiplexers (whose server is
/// not the window the user sees).
const BOUNDARY: &[&str] = &[
    "systemd",
    "init",
    "sddm",
    "gdm",
    "login",
    "sshd",
    "plasmashell",
    "kwin_wayland",
    "kwin_x11",
    "startplasma-way",
    "startplasma-x11",
    "gnome-shell",
    "Hyprland",
    "sway",
    "niri",
    "cosmic-comp",
    "krunner",
    "ksmserver",
    "kded6",
    "dbus-daemon",
    "dbus-broker",
];

fn is_multiplexer(comm: &str) -> bool {
    comm.starts_with("tmux") || comm == "screen" || comm == "zellij"
}

/// The window owner for a session's chain: the outermost process below a
/// [`BOUNDARY`], which is the terminal or editor in practice. `None` inside
/// a multiplexer, or with only the session process itself.
pub fn host<'a>(chain: &[&'a Proc]) -> Option<&'a Proc> {
    let mut host = None;
    for p in chain.iter().skip(1) {
        if is_multiplexer(&p.exe) {
            return None;
        }
        if BOUNDARY.contains(&p.exe.as_str()) {
            break;
        }
        host = Some(*p);
    }
    host
}

// ------------------------------------------------------------------- focus

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    Kde,
    Hyprland,
    Sway,
    Other,
}

fn desktop() -> Desktop {
    let var = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if current.split(':').any(|d| d.eq_ignore_ascii_case("KDE")) {
        Desktop::Kde
    } else if var("HYPRLAND_INSTANCE_SIGNATURE") {
        Desktop::Hyprland
    } else if var("SWAYSOCK") {
        Desktop::Sway
    } else {
        Desktop::Other
    }
}

/// The KWin script that focuses the first window owned by a PID in `pids`
/// (nearest the session first), preferring one whose caption holds the
/// project. Works on KWin 5 (`clientList`, `activeClient`) and 6.
pub fn kwin_script(pids: &[u32], project: &str) -> String {
    let pids = serde_json::to_string(pids).unwrap_or_else(|_| "[]".into());
    let project = serde_json::to_string(&project.to_lowercase()).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function () {{
  const pids = {pids};
  const project = {project};
  const all = workspace.windowList ? workspace.windowList() : workspace.clientList();
  for (const pid of pids) {{
    const mine = all.filter(function (w) {{ return w.pid === pid && w.normalWindow; }});
    if (!mine.length) continue;
    const best = (project && mine.find(function (w) {{
      return String(w.caption).toLowerCase().indexOf(project) >= 0;
    }})) || mine[0];
    if (best.minimized) best.minimized = false;
    if ("activeWindow" in workspace) workspace.activeWindow = best;
    else workspace.activeClient = best;
    return;
  }}
}})();
"#
    )
}

const KWIN_SCRIPT_NAME: &str = "knock-focus";

fn kwin_focus(pids: &[u32], project: &str) -> zbus::Result<()> {
    use zbus::blocking::{Connection, Proxy};
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join("knock-focus.js");
    std::fs::write(&path, kwin_script(pids, project))
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    let conn = Connection::session()?;
    let scripting = Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;
    let _: zbus::Result<bool> = scripting.call("unloadScript", &(KWIN_SCRIPT_NAME,));
    let id: i32 = scripting.call(
        "loadScript",
        &(path.to_string_lossy().as_ref(), KWIN_SCRIPT_NAME),
    )?;
    // KWin 6 names the script object /Scripting/Script<id>; KWin 5, /<id>.
    let mut ran = Err(zbus::Error::Failure("no script object".into()));
    for object in [format!("/Scripting/Script{id}"), format!("/{id}")] {
        let Ok(script) = Proxy::new(&conn, "org.kde.KWin", object, "org.kde.kwin.Script") else {
            continue;
        };
        ran = script.call::<_, _, ()>("run", &());
        if ran.is_ok() {
            break;
        }
    }
    let _: zbus::Result<bool> = scripting.call("unloadScript", &(KWIN_SCRIPT_NAME,));
    ran
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn hyprland_focus(pids: &[u32], project: &str) -> bool {
    let Some(json) = run("hyprctl", &["clients", "-j"]) else {
        return false;
    };
    let Ok(serde_json::Value::Array(clients)) = serde_json::from_str(&json) else {
        return false;
    };
    let project = project.to_lowercase();
    for pid in pids {
        let mine: Vec<_> = (clients.iter())
            .filter(|c| c["pid"].as_u64() == Some(u64::from(*pid)))
            .collect();
        let best = mine
            .iter()
            .find(|c| {
                !project.is_empty()
                    && c["title"]
                        .as_str()
                        .is_some_and(|t| t.to_lowercase().contains(&project))
            })
            .or(mine.first());
        if let Some(addr) = best.and_then(|c| c["address"].as_str()) {
            return run(
                "hyprctl",
                &["dispatch", "focuswindow", &format!("address:{addr}")],
            )
            .is_some();
        }
    }
    false
}

fn sway_focus(pids: &[u32]) -> bool {
    pids.iter()
        .any(|pid| run("swaymsg", &[&format!("[pid={pid}]"), "focus"]).is_some())
}

/// X11 fallback through `wmctrl`, if installed: `-lp` lists windows with
/// their PIDs.
fn wmctrl_focus(pids: &[u32]) -> bool {
    let Some(list) = run("wmctrl", &["-lp"]) else {
        return false;
    };
    for pid in pids {
        let win = list.lines().find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.get(2)? == &pid.to_string()).then(|| f[0].to_string())
        });
        if let Some(id) = win {
            return run("wmctrl", &["-ia", &id]).is_some();
        }
    }
    false
}

// -------------------------------------------------------------------- idle

/// ms since the epoch when the current idle period began, or 0 while the
/// user is active. Set from ext-idle-notify-v1 events.
static IDLE_SINCE: AtomicU64 = AtomicU64::new(0);
/// The Wayland idle watch is running.
static IDLE_WATCHED: AtomicBool = AtomicBool::new(false);
/// The compositor reports idle after this long without input; idle times
/// below it read as 0, which is far below the 10-minute away threshold.
const IDLE_NOTICE_MS: u32 = 30_000;

fn now_ms() -> u64 {
    crate::pipeline::now_ms()
}

mod idle {
    use super::{IDLE_NOTICE_MS, IDLE_SINCE, IDLE_WATCHED, now_ms};
    use std::sync::atomic::Ordering;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::ext::idle_notify::v1::client::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::ExtIdleNotifierV1,
    };

    struct State;

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(
            _: &mut Self,
            _: &wl_seat::WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<ExtIdleNotifierV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotifierV1,
            _: <ExtIdleNotifierV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<ExtIdleNotificationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let since = match event {
                ext_idle_notification_v1::Event::Idled => {
                    now_ms().saturating_sub(u64::from(IDLE_NOTICE_MS)).max(1)
                }
                _ => 0,
            };
            IDLE_SINCE.store(since, Ordering::Relaxed);
        }
    }

    /// Blocks: binds the notifier and dispatches its events forever.
    /// Returns only if the compositor lacks it or the connection fails.
    pub fn watch() -> Result<(), Box<dyn std::error::Error>> {
        let conn = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ())?;
        let notifier: ExtIdleNotifierV1 = globals.bind(&qh, 1..=2, ())?;
        // Version 2 ignores idle inhibitors (a playing video): the user is
        // away from the keyboard either way.
        let _n = if notifier.version() >= 2 {
            notifier.get_input_idle_notification(IDLE_NOTICE_MS, &seat, &qh, ())
        } else {
            notifier.get_idle_notification(IDLE_NOTICE_MS, &seat, &qh, ())
        };
        IDLE_WATCHED.store(true, Ordering::Relaxed);
        loop {
            queue.blocking_dispatch(&mut State)?;
        }
    }
}

fn start_idle_watch() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("knock-idle".into())
        .spawn(|| {
            if let Err(e) = idle::watch() {
                IDLE_WATCHED.store(false, Ordering::Relaxed);
                tracing::info!(error = %e, "wayland idle time not available");
            }
        });
}

/// D-Bus fallbacks for X11 and GNOME: Mutter's idle monitor, then the
/// freedesktop screensaver (KDE on X11 answers in ms).
fn dbus_idle_ms() -> Option<u64> {
    static CONN: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    let conn = CONN
        .get_or_init(|| zbus::blocking::Connection::session().ok())
        .as_ref()?;
    let call = |dest: &str, path: &str, iface: &str, method: &str| {
        zbus::blocking::Proxy::new(conn, dest.to_string(), path.to_string(), iface.to_string())
            .ok()?
            .call_method(method, &())
            .ok()
    };
    if let Some(reply) = call(
        "org.gnome.Mutter.IdleMonitor",
        "/org/gnome/Mutter/IdleMonitor/Core",
        "org.gnome.Mutter.IdleMonitor",
        "GetIdletime",
    ) {
        return reply.body().deserialize::<u64>().ok();
    }
    call(
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
        "GetSessionIdleTime",
    )
    .and_then(|r| r.body().deserialize::<u32>().ok())
    .map(u64::from)
}

// --------------------------------------------------------- launch at login

/// `~/.config/autostart/knock.desktop` (XDG autostart).
fn autostart_file() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| std::io::Error::other("HOME not set"))?;
    Ok(base.join("autostart").join("knock.desktop"))
}

/// The program Knock starts as: the AppImage itself when run from one
/// (`current_exe` is then inside a temporary mount), else this exe.
fn own_exe() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        Some(p) => Ok(PathBuf::from(p)),
        None => std::env::current_exe(),
    }
}

/// The `Exec=` value of an autostart file's text.
pub fn exec_line(desktop: &str) -> Option<&str> {
    desktop
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("Exec="))
        .map(str::trim)
}

pub fn autostart_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Knock\n\
         Comment=Your Claude Code sessions, living on your panel\n\
         Exec={exec}\nIcon=knock\nTerminal=false\nNoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

fn current_exec() -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(autostart_file()?) {
        Ok(text) => Ok(exec_line(&text).map(str::to_string)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn apply_run_ops(ops: &[RunOp], own: &str) -> std::io::Result<()> {
    let file = autostart_file()?;
    for op in ops {
        match op {
            RunOp::Write => {
                std::fs::create_dir_all(file.parent().unwrap_or(&file))?;
                crate::settings::write_atomic(&file, autostart_entry(own).as_bytes())?;
            }
            RunOp::Delete => match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
            // Writing the file afresh already drops a `Hidden=true` that a
            // settings app may have added; nothing else marks it disabled.
            RunOp::ClearApproved => {}
        }
    }
    Ok(())
}

fn sign_in_allowed() -> bool {
    let test_var = std::env::var_os("KNOCK_AUTOSTART_TEST");
    super::sign_in_allowed(cfg!(debug_assertions), test_var.as_deref())
}

// ---------------------------------------------------------------- Platform

impl Platform for LinuxPlatform {
    fn displays(&self) -> Vec<Display> {
        self.on_main(|| screens().into_iter().map(|(_, d, _)| d).collect())
            .unwrap_or_default()
    }

    fn taskbar(&self) -> Option<Taskbar> {
        // Layer shell keeps the strip clear of panels by itself, and on X11
        // the work area already leaves them out.
        None
    }

    fn strip_rect(&self) -> Option<Rect> {
        *self.placed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn watch_displays(&self, changed: Arc<AtomicBool>) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let flag = move || changed.store(true, Ordering::Relaxed);
        let f = flag.clone();
        display.connect_monitor_added(move |_, _| f());
        let f = flag.clone();
        display.connect_monitor_removed(move |_, _| f());
        if let Some(screen) = gdk::Screen::default() {
            let f = flag.clone();
            screen.connect_size_changed(move |_| f());
            let f = flag.clone();
            screen.connect_monitors_changed(move |_| f());
        }
        for i in 0..display.n_monitors() {
            if let Some(m) = display.monitor(i) {
                let f = flag.clone();
                m.connect_scale_factor_notify(move |_| f());
                let f = flag.clone();
                m.connect_workarea_notify(move |_| f());
            }
        }
    }

    fn place_strip(&self, r: Rect) -> std::io::Result<()> {
        if self.backend == Backend::None {
            return Ok(());
        }
        let backend = self.backend;
        let regions = self
            .regions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let placed = self.on_main(move || {
            let (d, monitor) = monitor_for(r)?;
            let logical = |px: i32| (f64::from(px) / d.scale).round() as i32;
            let (w, h) = (
                logical(r.right - r.left).max(1),
                logical(r.bottom - r.top).max(1),
            );
            with_strip(|window| {
                match backend {
                    Backend::LayerShell => {
                        use gtk_layer_shell::Edge;
                        LayerShell::set_monitor(window, &monitor);
                        // Margins from the monitor's own edges; the
                        // compositor adds any panel on top.
                        let right = logical(d.monitor.right - r.right);
                        let bottom = logical(d.monitor.bottom - r.bottom);
                        LayerShell::set_layer_shell_margin(window, Edge::Right, right);
                        LayerShell::set_layer_shell_margin(window, Edge::Bottom, bottom);
                    }
                    _ => window.move_(logical(r.left), logical(r.top)),
                }
                window.set_size_request(w, h);
                window.resize(w, h);
                apply_input_region(window, &regions);
            })
        });
        match placed.flatten() {
            Some(()) => {
                *self.placed.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                Ok(())
            }
            None => Err(std::io::Error::other("strip window not available")),
        }
    }

    fn show_strip(&self) {
        self.shown.store(true, Ordering::Relaxed);
        let hidden = *self.layer.lock().unwrap_or_else(|e| e.into_inner()) == StripLayer::Hidden;
        if self.backend == Backend::None || hidden {
            return;
        }
        let regions = self
            .regions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.on_main(move || {
            with_strip(|window| {
                window.show_all();
                apply_input_region(window, &regions);
            })
        });
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        None
    }

    fn set_strip_layer(&self, layer: StripLayer) {
        *self.layer.lock().unwrap_or_else(|e| e.into_inner()) = layer;
        if self.backend == Backend::None {
            return;
        }
        let (backend, shown) = (self.backend, self.shown.load(Ordering::Relaxed));
        let regions = self
            .regions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.on_main(move || {
            with_strip(|w| {
                let top = match layer {
                    StripLayer::Hidden => return w.hide(),
                    StripLayer::Top => true,
                    StripLayer::Desktop => false,
                };
                match backend {
                    // Bottom: above the wallpaper and desktop icons, below
                    // every normal window.
                    Backend::LayerShell => LayerShell::set_layer(
                        w,
                        if top {
                            gtk_layer_shell::Layer::Top
                        } else {
                            gtk_layer_shell::Layer::Bottom
                        },
                    ),
                    _ => {
                        w.set_keep_below(!top);
                        w.set_keep_above(top);
                    }
                }
                if shown && !w.is_visible() {
                    w.show_all();
                    apply_input_region(w, &regions);
                }
            })
        });
        tracing::info!(?layer, "strip layer");
    }

    fn set_click_through(&self, on: bool) {
        // Only "everything passes through" has a meaning here; the hit
        // regions decide the rest.
        if on {
            self.set_hit_regions(&[]);
        }
    }

    fn set_hit_regions(&self, regions: &[Region]) {
        let regions = regions.to_vec();
        *self.regions.lock().unwrap_or_else(|e| e.into_inner()) = regions.clone();
        self.on_main(move || with_strip(|w| apply_input_region(w, &regions)));
    }

    fn connection_owner(&self, client_port: u16, server_port: u16) -> Option<ProcessId> {
        let table = std::fs::read_to_string("/proc/net/tcp").ok()?;
        let pid = socket_owner(tcp_inode(&table, client_port, server_port)?)?;
        Some(ProcessId {
            pid,
            start: process_start(pid)?,
        })
    }

    fn find_window(&self, process: ProcessId, project: &str) -> Option<Target> {
        let procs = processes();
        let chain = jump::chain(&procs, process);
        let host = host(&chain)?;
        let pids: Vec<u32> = chain.iter().map(|p| p.pid).collect();
        let token = process.pid as isize;
        (self.jumps.lock().unwrap_or_else(|e| e.into_inner()))
            .insert(token, (pids, project.to_string()));
        Some(Target {
            window: token,
            owner: host.exe.clone(),
            precision: Precision::AppWindow,
        })
    }

    fn focus_window(&self, window: isize) -> bool {
        let Some((pids, project)) = self
            .jumps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&window)
            .cloned()
        else {
            return false;
        };
        let desktop = desktop();
        // Off the main thread: D-Bus and helper programs must never stall
        // the UI (§9.6). The result is only logged.
        let _ = std::thread::Builder::new()
            .name("knock-focus".into())
            .spawn(move || {
                let ok = match desktop {
                    Desktop::Kde => kwin_focus(&pids, &project)
                        .map_err(|e| tracing::info!(error = %e, "kwin focus failed"))
                        .is_ok(),
                    Desktop::Hyprland => hyprland_focus(&pids, &project),
                    Desktop::Sway => sway_focus(&pids),
                    Desktop::Other => wmctrl_focus(&pids),
                };
                tracing::info!(?desktop, ok, "focus");
            });
        desktop != Desktop::Other || std::env::var_os("DISPLAY").is_some()
    }

    fn process_start(&self, pid: u32) -> Option<u64> {
        process_start(pid)
    }

    fn monitors(&self) -> Vec<Monitor> {
        self.on_main(|| screens().into_iter().map(|(m, _, _)| m).collect())
            .unwrap_or_default()
    }

    fn set_launch_at_sign_in(&self, on: bool) -> std::io::Result<SignInChange> {
        if !sign_in_allowed() {
            tracing::info!(on, "debug build: autostart entry not changed");
            return Ok(SignInChange::default());
        }
        let own = super::run_value(&own_exe()?);
        let current = if on { None } else { current_exec()? };
        let change = super::sign_in_change(on, current.as_deref(), &own);
        apply_run_ops(change.now, &own)?;
        Ok(change)
    }

    fn apply_sign_in_ops(&self, ops: &[RunOp]) -> std::io::Result<()> {
        if ops.is_empty() || !sign_in_allowed() {
            return Ok(());
        }
        apply_run_ops(ops, &super::run_value(&own_exe()?))
    }

    fn restore_launch_at_sign_in(&self, setting: Option<bool>) -> std::io::Result<bool> {
        if !sign_in_allowed() {
            return Ok(false);
        }
        let own = super::run_value(&own_exe()?);
        let ops = super::start_ops(setting, current_exec()?.as_deref(), &own);
        apply_run_ops(ops, &own)?;
        Ok(!ops.is_empty())
    }

    fn idle_ms(&self) -> Option<u64> {
        if IDLE_WATCHED.load(Ordering::Relaxed) {
            let since = IDLE_SINCE.load(Ordering::Relaxed);
            return Some(match since {
                0 => 0,
                s => now_ms().saturating_sub(s),
            });
        }
        dbus_idle_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_fields_are_read_after_the_last_parenthesis() {
        let line =
            "4242 (my (odd) cmd) S 100 4242 4242 0 -1 4194304 1 2 3 4 5 6 7 8 20 0 1 0 987654 1 2";
        let (parent, start, comm) = parse_stat(line).unwrap();
        assert_eq!(
            (parent, start, comm.as_str()),
            (100, 987654, "my (odd) cmd")
        );
        assert!(parse_stat("garbage").is_none());
    }

    #[test]
    fn this_process_stat_parses() {
        let pid = std::process::id();
        let (_, start, _) = read_stat(pid).unwrap();
        assert_eq!(process_start(pid), Some(start));
    }

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:C350 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000000000000000 100 0 0 10 0
   1: 0100007F:9C40 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 20 4 30 10 -1
   2: 0100007F:C350 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 33333 1 0000000000000000 20 4 30 10 -1
";

    #[test]
    fn tcp_inode_finds_the_client_end() {
        // 0x9C40 = 40000 (client), 0xC350 = 50000 (server).
        assert_eq!(tcp_inode(TCP, 40000, 50000), Some(22222));
        assert_eq!(tcp_inode(TCP, 50000, 40000), Some(33333));
        assert_eq!(tcp_inode(TCP, 1, 50000), None);
    }

    #[test]
    fn connection_owner_of_this_process() {
        use std::net::{Ipv4Addr, TcpListener, TcpStream};
        let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(server.local_addr().unwrap()).unwrap();
        let table = std::fs::read_to_string("/proc/net/tcp").unwrap();
        let inode = tcp_inode(
            &table,
            client.local_addr().unwrap().port(),
            server.local_addr().unwrap().port(),
        )
        .unwrap();
        assert_eq!(socket_owner(inode), Some(std::process::id()));
    }

    fn proc(pid: u32, parent: u32, exe: &str) -> Proc {
        Proc {
            pid,
            parent,
            start: Some(1),
            exe: exe.into(),
        }
    }

    #[test]
    fn host_is_the_outermost_process_below_the_desktop() {
        let p = [
            proc(10, 9, "claude"),
            proc(9, 8, "fish"),
            proc(8, 1, "konsole"),
            proc(1, 0, "systemd"),
        ];
        let chain: Vec<&Proc> = p.iter().collect();
        assert_eq!(host(&chain).unwrap().exe, "konsole");
        // VS Code: the editor's main process, past its helpers.
        let p = [
            proc(10, 9, "claude"),
            proc(9, 8, "bash"),
            proc(8, 7, "code"),
            proc(7, 2, "code"),
            proc(2, 1, "plasmashell"),
        ];
        let chain: Vec<&Proc> = p.iter().collect();
        assert_eq!(host(&chain).unwrap().pid, 7);
        // Inside tmux: no window to find.
        let p = [
            proc(10, 9, "claude"),
            proc(9, 8, "bash"),
            proc(8, 1, "tmux: server"),
        ];
        let chain: Vec<&Proc> = p.iter().collect();
        assert!(host(&chain).is_none());
        // Only the session itself.
        assert!(host(&[&proc(10, 1, "claude")]).is_none());
    }

    #[test]
    fn kwin_script_embeds_pids_and_project_as_json() {
        let s = kwin_script(&[10, 8], r#"My "Proj""#);
        assert!(s.contains("const pids = [10,8];"));
        assert!(s.contains(r#"const project = "my \"proj\"";"#));
    }

    #[test]
    fn autostart_entry_round_trips_its_exec() {
        let own = r#""/home/a b/Applications/Knock.AppImage""#;
        assert_eq!(exec_line(&autostart_entry(own)), Some(own));
        assert_eq!(exec_line("[Desktop Entry]\nName=x\n"), None);
        // An old copy of Knock, anywhere, counts as Knock's (ADR 0025).
        let c = super::super::sign_in_change(
            false,
            Some("/opt/knock/knock-desktop"),
            r#""/usr/bin/knock-desktop""#,
        );
        assert_eq!(c.now, [RunOp::Delete]);
    }

    #[test]
    fn local_minute_is_within_a_day() {
        assert!(local_minute() < 24 * 60);
    }

    #[test]
    fn random_bytes_differ() {
        let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
        fill_random(&mut a).unwrap();
        fill_random(&mut b).unwrap();
        assert_ne!(a, b);
    }
}
