//! Pure session-to-window logic (SPEC.md §9.4, §9.5). No OS calls: the
//! platform layer takes the snapshots, these functions decide. Unit-tested on
//! process trees and window lists recorded on the user's machine.
//!
//! The window-list and TCP-table parts serve the Windows platform only; they
//! stay compiled (and tested) everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;
use std::net::Ipv4Addr;

/// A process, told apart from a later one with a reused PID by its start
/// time (§9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessId {
    pub pid: u32,
    /// Creation time as a Windows FILETIME (100 ns ticks).
    pub start: u64,
}

/// One row of a process snapshot.
#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// `None` when the process can't be opened (another user, or protected).
    pub start: Option<u64>,
    /// File name only, never a path.
    pub exe: String,
}

/// A top-level window owned by a process in the chain, described by its root
/// owner (`GA_ROOTOWNER`): that is the window Knock focuses. A Windows
/// Terminal tab's `PseudoConsoleWindow` belongs to the shell, but its root
/// owner is the terminal window that shows it.
#[derive(Clone, Debug)]
pub struct Win {
    pub pid: u32,
    pub root: isize,
    pub root_pid: u32,
    pub root_visible: bool,
    pub root_class: String,
    /// Root title contains the project name (case-insensitive). The title
    /// itself is never kept.
    pub root_has_project: bool,
}

/// §9.5 jump precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    ExactWindow,
    AppWindow,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub window: isize,
    /// Executable that owns `window`, file name only.
    pub owner: String,
    pub precision: Precision,
}

/// Deep enough for any real host; stops a cycle of equal start times.
const MAX_DEPTH: usize = 32;
const CONSOLE_CLASS: &str = "ConsoleWindowClass";
/// ConPTY's stand-in console window: never a real target on its own.
const PSEUDO_CONSOLE_CLASS: &str = "PseudoConsoleWindow";

/// `of` and its parents, nearest first. Empty if `of` is gone or its PID now
/// belongs to a newer process. Stops at a parent that started after its child
/// (its PID was reused, so the real parent is gone) or whose start time can't
/// be read.
pub fn chain(procs: &[Proc], of: ProcessId) -> Vec<&Proc> {
    let find = |pid: u32| procs.iter().find(|p| p.pid == pid);
    let Some(mut cur) = find(of.pid).filter(|p| p.start == Some(of.start)) else {
        return Vec::new();
    };
    let mut out = vec![cur];
    while out.len() < MAX_DEPTH {
        let parent = find(cur.parent).filter(|p| {
            p.pid != cur.pid && p.start.zip(cur.start).is_some_and(|(ps, cs)| ps <= cs)
        });
        let Some(parent) = parent else { break };
        out.push(parent);
        cur = parent;
    }
    out
}

/// Processes that only pass a command on, never the agent: shells and the
/// like (names without `.exe`, lower case).
const WRAPPERS: [&str; 17] = [
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "ash",
    "ksh",
    "tcsh",
    "csh",
    "env",
    "timeout",
    "busybox",
    "cmd",
    "powershell",
    "pwsh",
    "conhost",
    "nohup",
];

/// The process that runs an agent's hook helper (ADR 0039): the nearest
/// ancestor of `helper` that isn't a shell or similar wrapper. A hook command
/// is started by the agent, directly or through `sh -c`; either way this is the
/// agent, which lives as long as the session. `None` if there is none or its
/// start time can't be read.
pub fn agent_of(procs: &[Proc], helper: ProcessId) -> Option<ProcessId> {
    let is_wrapper = |p: &Proc| {
        let name = p.exe.to_ascii_lowercase();
        WRAPPERS.contains(&name.trim_end_matches(".exe"))
    };
    let chain = chain(procs, helper);
    let agent = chain.iter().skip(1).find(|p| !is_wrapper(p))?;
    Some(ProcessId {
        pid: agent.pid,
        start: agent.start?,
    })
}

/// §9.4 step 2 and §9.5: the first process in `chain` that owns a visible
/// top-level window (through its root owner) gives the target. Among several,
/// one whose title contains the project wins, else the first (topmost).
/// Explorer ends the walk: its windows are the taskbar and desktop, never a
/// host.
pub fn pick(procs: &[Proc], chain: &[&Proc], wins: &[Win]) -> Option<Target> {
    for p in chain {
        if p.exe.eq_ignore_ascii_case("explorer.exe") {
            break;
        }
        let candidates = || {
            wins.iter().filter(|w| {
                w.pid == p.pid && w.root_visible && w.root_class != PSEUDO_CONSOLE_CLASS
            })
        };
        let Some(w) = candidates()
            .find(|w| w.root_has_project)
            .or_else(|| candidates().next())
        else {
            continue;
        };
        let owner = procs
            .iter()
            .find(|q| q.pid == w.root_pid)
            .map_or_else(String::new, |q| q.exe.clone());
        let precision = if w.root_class == CONSOLE_CLASS {
            Precision::ExactWindow
        } else {
            Precision::AppWindow
        };
        return Some(Target {
            window: w.root,
            owner,
            precision,
        });
    }
    None
}

/// One `MIB_TCPROW_OWNER_PID`: state, local addr, local port, remote addr,
/// remote port, owning PID. Addresses and ports are in network byte order;
/// a port uses only the low 16 bits.
pub type TcpRow = [u32; 6];

/// §9.4 step 1: the PID owning the client end of the loopback connection from
/// `client_port` to `server_port`, from a TCP connection table.
pub fn tcp_owner(rows: &[TcpRow], client_port: u16, server_port: u16) -> Option<u32> {
    let port = |v: u32| u16::from_be(v as u16);
    let loopback = |v: u32| Ipv4Addr::from(v.to_ne_bytes()) == Ipv4Addr::LOCALHOST;
    rows.iter()
        .find(|r| {
            loopback(r[1])
                && port(r[2]) == client_port
                && loopback(r[3])
                && port(r[4]) == server_port
        })
        .map(|r| r[5])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: u32, start: Option<u64>, exe: &str) -> Proc {
        Proc {
            pid,
            parent,
            start,
            exe: exe.into(),
        }
    }

    fn win(pid: u32, root: isize, root_pid: u32, visible: bool, class: &str) -> Win {
        Win {
            pid,
            root,
            root_pid,
            root_visible: visible,
            root_class: class.into(),
            root_has_project: false,
        }
    }

    fn id(pid: u32, start: u64) -> ProcessId {
        ProcessId { pid, start }
    }

    fn exes(c: &[&Proc]) -> Vec<String> {
        c.iter().map(|p| p.exe.clone()).collect()
    }

    // Recorded 2026-10-18 on the user's machine (Windows 11 25H2). PIDs and
    // exe names are real; start times are shortened to seconds, in order.
    //
    // Classic console, `conhost powershell` started from a shell under VS
    // Code: Knock logged this chain for a real `claude -p` run. The window
    // list is from a diagnostic dump of the same kind of console: the
    // console window is reported as owned by the shell, not by conhost.
    fn classic() -> (Vec<Proc>, Vec<Win>) {
        let procs = vec![
            proc(7596, 1900, Some(185), "claude.exe"),
            proc(1900, 2504, Some(180), "powershell.exe"),
            proc(2504, 18456, Some(179), "conhost.exe"),
            proc(18456, 17336, Some(177), "powershell.exe"),
            proc(17336, 16056, Some(177), "cmd.exe"),
        ];
        let wins = vec![win(1900, 0x2E0424, 1900, true, "ConsoleWindowClass")];
        (procs, wins)
    }

    // Windows Terminal 1.24, interactive `claude` (chain logged by Knock).
    // The shell owns a visible PseudoConsoleWindow whose root owner is the
    // terminal window showing its tab. One WindowsTerminal.exe hosted three
    // windows; the session's tab was in 0x70936, listed last here.
    fn terminal() -> (Vec<Proc>, Vec<Win>) {
        let procs = vec![
            proc(9116, 16184, Some(1500), "claude.exe"),
            proc(16184, 8212, Some(1500), "powershell.exe"),
            proc(8212, 20424, Some(523), "WindowsTerminal.exe"),
        ];
        let wins = vec![
            win(16184, 0x70936, 8212, true, "CASCADIA_HOSTING_WINDOW_CLASS"),
            win(8212, 0x1C06FA, 8212, true, "CASCADIA_HOSTING_WINDOW_CLASS"),
            win(8212, 0x18075E, 8212, true, "CASCADIA_HOSTING_WINDOW_CLASS"),
            win(8212, 0x70936, 8212, true, "CASCADIA_HOSTING_WINDOW_CLASS"),
            win(8212, 0x607EE, 8212, false, "OleDdeWndClass"),
        ];
        (procs, wins)
    }

    // VS Code 1.142 integrated terminal, from a diagnostic dump of a real
    // terminal shell (claude was not run in it; PID 30000 stands in). The
    // shell's PseudoConsoleWindow is hidden and unowned; the window belongs
    // to the main Code.exe, two levels up past the pty host.
    fn vscode() -> (Vec<Proc>, Vec<Win>) {
        let procs = vec![
            proc(30000, 18284, Some(200), "claude.exe"),
            proc(18284, 19056, Some(190), "cmd.exe"),
            proc(19056, 15852, Some(150), "Code.exe"),
            proc(15852, 4480, Some(149), "Code.exe"),
        ];
        let wins = vec![
            win(18284, 0x1406D6, 18284, false, "PseudoConsoleWindow"),
            win(15852, 0x4056E, 15852, true, "Chrome_WidgetWin_1"),
            win(15852, 0x2B04DC, 15852, false, "Chrome_WidgetWin_0"),
        ];
        (procs, wins)
    }

    fn resolve(procs: &[Proc], wins: &[Win], of: ProcessId) -> Option<Target> {
        pick(procs, &chain(procs, of), wins)
    }

    #[test]
    fn a_hook_helper_belongs_to_the_agent_above_it() {
        let procs = [
            proc(900, 800, Some(300), "knock-desktop"),
            proc(800, 700, Some(290), "sh"),
            proc(700, 600, Some(100), "codex"),
            proc(600, 500, Some(90), "bash"),
            proc(500, 1, Some(80), "konsole"),
        ];
        let helper = id(900, 300);
        // Through `sh -c`: the shell is skipped, the agent is found.
        assert_eq!(agent_of(&procs, helper), Some(id(700, 100)));
        // Started directly by the agent.
        let direct = [proc(900, 700, Some(300), "knock-desktop"), procs[2].clone()];
        assert_eq!(agent_of(&direct, helper), Some(id(700, 100)));
        // Windows names, and wrappers stacked on top of each other.
        let win = [
            proc(900, 800, Some(300), "knock-desktop.exe"),
            proc(800, 750, Some(290), "CMD.EXE"),
            proc(750, 700, Some(280), "conhost.exe"),
            proc(700, 1, Some(100), "node.exe"),
        ];
        assert_eq!(agent_of(&win, helper), Some(id(700, 100)));
        // Only shells above it, an unknown helper, or an unreadable start: none.
        assert_eq!(agent_of(&procs[..2], helper), None);
        assert_eq!(agent_of(&procs, id(901, 300)), None);
        let mut blind = procs.to_vec();
        blind[2].start = None;
        assert_eq!(agent_of(&blind, helper), None);
    }

    #[test]
    fn classic_console_is_exact_window() {
        let (procs, wins) = classic();
        let c = chain(&procs, id(7596, 185));
        assert_eq!(
            exes(&c),
            [
                "claude.exe",
                "powershell.exe",
                "conhost.exe",
                "powershell.exe",
                "cmd.exe"
            ]
        );
        let t = pick(&procs, &c, &wins).unwrap();
        assert_eq!(t.window, 0x2E0424);
        assert_eq!(t.owner, "powershell.exe");
        assert_eq!(t.precision, Precision::ExactWindow);
    }

    #[test]
    fn windows_terminal_is_app_window_through_the_root_owner() {
        let (procs, wins) = terminal();
        // The tab's window, not the terminal process's first window.
        let t = resolve(&procs, &wins, id(9116, 1500)).unwrap();
        assert_eq!(t.window, 0x70936);
        assert_eq!(t.owner, "WindowsTerminal.exe");
        assert_eq!(t.precision, Precision::AppWindow);
    }

    // Recorded: PowerShell started through Explorer with "Let Windows decide"
    // as the default terminal is handed to Windows Terminal. Its parent is
    // Explorer, so only the root owner leads to the terminal window.
    #[test]
    fn default_terminal_handoff_uses_the_root_owner_not_explorer() {
        let procs = vec![
            proc(5000, 6032, Some(600), "claude.exe"),
            proc(6032, 19228, Some(563), "powershell.exe"),
            proc(19228, 1604, Some(563), "explorer.exe"),
            proc(8212, 20424, Some(523), "WindowsTerminal.exe"),
        ];
        let wins = vec![
            win(6032, 0x3A074A, 8212, true, "CASCADIA_HOSTING_WINDOW_CLASS"),
            win(19228, 0x1000C, 1300, true, "Shell_TrayWnd"),
        ];
        let t = resolve(&procs, &wins, id(5000, 600)).unwrap();
        assert_eq!(
            (t.window, t.owner.as_str()),
            (0x3A074A, "WindowsTerminal.exe")
        );
    }

    #[test]
    fn explorer_ends_the_walk() {
        let procs = vec![
            proc(5000, 6032, Some(600), "claude.exe"),
            proc(6032, 19228, Some(563), "powershell.exe"),
            proc(19228, 1604, Some(563), "Explorer.EXE"),
        ];
        let wins = vec![win(19228, 0x1000C, 19228, true, "Shell_TrayWnd")];
        assert_eq!(resolve(&procs, &wins, id(5000, 600)), None);
    }

    #[test]
    fn vscode_skips_the_hidden_pseudo_console_and_the_pty_host() {
        let (procs, wins) = vscode();
        let t = resolve(&procs, &wins, id(30000, 200)).unwrap();
        assert_eq!(t.window, 0x4056E);
        assert_eq!(t.owner, "Code.exe");
        assert_eq!(t.precision, Precision::AppWindow);
    }

    #[test]
    fn a_visible_unowned_pseudo_console_is_never_picked() {
        let (procs, mut wins) = vscode();
        wins[0].root_visible = true;
        let t = resolve(&procs, &wins, id(30000, 200)).unwrap();
        assert_eq!(t.window, 0x4056E);
    }

    #[test]
    fn editor_window_with_the_project_in_its_title_wins() {
        let (procs, mut wins) = vscode();
        wins.push(win(15852, 0x7777, 15852, true, "Chrome_WidgetWin_1"));
        // Without a title match, the first (topmost) window.
        assert_eq!(
            resolve(&procs, &wins, id(30000, 200)).unwrap().window,
            0x4056E
        );
        wins.last_mut().unwrap().root_has_project = true;
        assert_eq!(
            resolve(&procs, &wins, id(30000, 200)).unwrap().window,
            0x7777
        );
    }

    #[test]
    fn nothing_found_is_none() {
        let (procs, _) = vscode();
        assert_eq!(resolve(&procs, &[], id(30000, 200)), None);
    }

    #[test]
    fn reused_pid_for_the_session_process_gives_nothing() {
        let (procs, wins) = classic();
        // PID 7596 now belongs to a process that started later.
        assert!(chain(&procs, id(7596, 100)).is_empty());
        assert_eq!(resolve(&procs, &wins, id(7596, 100)), None);
        // And a PID that is gone entirely.
        assert!(chain(&procs, id(4242, 185)).is_empty());
    }

    #[test]
    fn reused_parent_pid_ends_the_walk() {
        let (mut procs, wins) = classic();
        // The shell exited and its PID went to a process newer than claude.
        procs[1] = proc(1900, 4, Some(999), "notepad.exe");
        let c = chain(&procs, id(7596, 185));
        assert_eq!(exes(&c), ["claude.exe"]);
        assert_eq!(pick(&procs, &c, &wins), None);
    }

    #[test]
    fn unreadable_parent_ends_the_walk() {
        let (mut procs, _) = classic();
        procs[2].start = None;
        let c = chain(&procs, id(7596, 185));
        assert_eq!(exes(&c), ["claude.exe", "powershell.exe"]);
    }

    #[test]
    fn walk_is_bounded_on_a_cycle() {
        let procs = vec![proc(1, 2, Some(5), "a.exe"), proc(2, 1, Some(5), "b.exe")];
        assert_eq!(chain(&procs, id(1, 5)).len(), MAX_DEPTH);
    }

    const fn be(port: u16) -> u32 {
        port.to_be() as u32
    }
    const LO: u32 = u32::from_ne_bytes([127, 0, 0, 1]);

    #[test]
    fn tcp_owner_finds_the_client_end() {
        let rows = [
            // Knock's own server end: local 62451, remote 50001.
            [5, LO, be(62451), LO, be(50001), 111],
            // Another loopback client to a different server.
            [5, LO, be(50002), LO, be(8080), 333],
            // The hook client: local 50001, remote 62451.
            [5, LO, be(50001), LO, be(62451), 222],
        ];
        assert_eq!(tcp_owner(&rows, 50001, 62451), Some(222));
        assert_eq!(tcp_owner(&rows, 50009, 62451), None);
        // Non-loopback addresses never match.
        let lan = u32::from_ne_bytes([192, 168, 1, 5]);
        assert_eq!(
            tcp_owner(&[[5, lan, be(50001), LO, be(62451), 9]], 50001, 62451),
            None
        );
    }

    #[test]
    fn precision_serializes_as_in_the_spec() {
        let s = serde_json::to_string(&[
            Precision::ExactWindow,
            Precision::AppWindow,
            Precision::None,
        ]);
        assert_eq!(s.unwrap(), r#"["exact_window","app_window","none"]"#);
    }
}
