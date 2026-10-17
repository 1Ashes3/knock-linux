# 0013: Bringing the session's window to the front

- **Status:** Proposed
- **Date:** 2026-10-18
- **Spec sections:** §9.6
- **VERIFY item:** V-9

## Context

§9.6 says Phase 0 must find a supported way to bring the target window to the front after a click on the non-activating strip, and must never simulate input. Spike C tried four options:

- `SetForegroundWindow`
- `AllowSetForegroundWindow`, as a diagnostic
- UI Automation `SetFocus`, which brought windows forward even without a click, so in effect it gets around Windows' foreground lock
- `FlashWindowEx`, as the fallback

## Decision

- On a click, call `SetForegroundWindow` on the target window. Then, if the target was minimized, restore it with `ShowWindowAsync(SW_RESTORE)`.
- If `SetForegroundWindow` returns false, flash the target's taskbar button with `FlashWindowEx`.
- Remove the UI Automation fallback and the `AllowSetForegroundWindow` call. Neither is needed.
- Never simulate keyboard or mouse input (§9.6).

## Consequences

- `core-windows` removes the Phase 0 diagnostics from `focus_window`: the UIA fallback, `AllowSetForegroundWindow`, the `KNOCK_NO_UIA` switch, the 200 ms foreground poll, and the per-attempt log fields. It also drops the `Win32_System_Com` and `Win32_UI_Accessibility` features.
- SPEC.md §9.6: "Phase 0 MUST find a supported way… (**VERIFY V-9**)" becomes "Call `SetForegroundWindow` from the click, restoring a minimized target; if it fails, flash the taskbar button (ADR 0013)."
- **Risk:** this was tested on one machine. The Spike C agent saw that this machine's runtime foreground-lock timeout differs from its registry value, and other machines may be stricter. The G2 tester round (§14) checks jumping on testers' machines. If it fails there, the flash fallback still applies, and this ADR is revisited.

## Evidence

**Setup:** Windows 11 Pro 10.0.26200.7462, Spike C debug build. The user clicked the strip's test square with real mouse clicks on 2026-10-17. Notepad was in front and being typed in before each click.

**First two passes (dd87727, 712fdbb):**
- Every jump succeeded: classic console, Windows Terminal and VS Code, each both normal and minimized.
- The log showed `set_foreground=false` with the UIA fallback doing the work. But the code checked `GetForegroundWindow` straight after `SetForegroundWindow`. Activating another thread's window happens asynchronously, so that check was too early.

**Decisive pass (a67b3a5):**
- Run with the UIA fallback turned off (`KNOCK_NO_UIA=1`), waiting up to 200 ms for activation.
- The user saw every window come to the front: classic console normal and minimized, Windows Terminal, and VS Code.
- All 4 log lines show `set_returned=true set_foreground=true no_uia=true front=true flashed=false`.

**Without a click:** in automated runs with no human click, `SetForegroundWindow` was usually refused. This matches Windows' documented rules: the click is what grants the right.
