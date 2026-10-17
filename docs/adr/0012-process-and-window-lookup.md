# 0012: Finding a session's process and window

- **Status:** Proposed
- **Date:** 2026-10-18
- **Spec sections:** §9.4, §9.5
- **VERIFY item:** V-7, V-8

## Context

§9.4 has two steps:

1. Find the process that owns the client end of the hook connection (V-7).
2. Walk up the parent processes to the window to focus (V-8).

Spike C tested both on the user's machine with a classic console, Windows Terminal and VS Code's terminal.

## Decision

- **V-7:** on a session's first request, before replying, look up the owner of the client's loopback connection in the TCP table (`GetExtendedTcpTable` with owner PIDs). The owner is `claude.exe` itself. Record its PID and start time.
- **V-8:** walk up from that process. Map every top-level window to its root owner (`GA_ROOTOWNER`). The first process in the chain that owns a visible root window gives the target. Three rules are added to §9.4 step 2:
  - Never pick a `PseudoConsoleWindow`. In Windows Terminal the shell owns one, and its root owner is the terminal window.
  - Stop the walk at `explorer.exe`, so a handed-off shell can never land on the taskbar.
  - If several windows qualify, prefer one whose title contains the project name, else the topmost.
- **Precision:** a `ConsoleWindowClass` window is `exact_window`; any other window is `app_window`; nothing found is `none` (§9.5).
- **Re-check on jump:** the window is looked up again on every jump, because it may have closed or moved.
- **No command hook:** the §9.4 step 3 command-hook fallback is not needed.

## Consequences

- SPEC.md §9.4 step 2: add "Map windows to their root owner; never pick a `PseudoConsoleWindow`; stop at `explorer.exe`; prefer a title containing the project (ADR 0012)."
- §9.4 step 3 (the fallback) is not adopted. ADR 0005 (no `source`) stands.
- Not tested: JetBrains IDEs and other editors. They fall under the `app_window` rule and are checked in the §12.2 release matrix.

## Evidence

**Setup:** Windows 11 Pro 10.0.26200.7462, Claude Code 2.1.302, Windows Terminal 1.24.12741.0, VS Code 1.142.0, Spike C debug build (dd87727 and later). Test project `%LOCALAPPDATA%\Knock\phase0-project`.

**V-7, finding the process:**
- Automated: 5 of 5 runs found `claude.exe`. That covered `-p` and interactive sessions in a classic console and Windows Terminal, plus one session handed off to Windows Terminal. Lookup took 157-408 µs, and the first reply 191-457 µs, so the §6 budget of under 100 ms holds.
- User runs on 2026-10-17: the log shows `claude.exe` found in every session, with lookups of 164-488 µs.

**V-8, finding the window.** The process chains in the log list executable names only. Each host was checked by the user (two passes, plus the run without the UIA fallback):

| Host | Chain | Owner | Precision |
| --- | --- | --- | --- |
| Classic console (`conhost powershell`) | `claude.exe`, `powershell.exe`, `conhost.exe`, `explorer.exe` | `powershell.exe` | `exact_window` |
| Windows Terminal | `claude.exe`, `powershell.exe`, `WindowsTerminal.exe`, `explorer.exe` | `WindowsTerminal.exe` | `app_window` |
| VS Code terminal | `claude.exe`, `cmd.exe`, `Code.exe`, `Code.exe` | `Code.exe` | `app_window` |

- In the automated tests, one `WindowsTerminal.exe` hosted 3 windows, and mapping to the root owner picked the session's own window. Walking by process alone would have been ambiguous.
- 16 unit tests on recorded process trees cover these rules, including PID reuse checked by start time.
