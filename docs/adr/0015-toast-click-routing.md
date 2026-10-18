# 0015: Clicking a toast jumps only while it is on screen

- **Status:** Proposed
- **Date:** 2026-10-18
- **Spec sections:** §9.7 (Notifications), §9.6
- **VERIFY item:** V-12

## Context

V-12 asks whether a click on a Windows toast can be routed back to Knock to jump to the session. The Tauri notification plugin (2.7.0) reports clicks through WinRT `ToastNotification.Activated`. That event fires only while the toast is on screen. A click on the same notification in the notification center (Win+N) needs a COM activator: `INotificationActivationCallback`, plus an installer shortcut that carries `ToastActivatorCLSID` and the app identity. The plugin doesn't provide that.

## Decision

- A click on the on-screen toast jumps to the session, using the same jump as the strip (ADR 0013).
- A click in the notification center only informs. It does nothing else, which is §9.7's fallback.
- No COM activator in v1. Revisit after the G2 testers if they ask for it.

## Consequences

- SPEC.md §9.7 Notifications: "Whether clicking a toast can jump to the session is **VERIFY V-12**; if not, the toast only informs" becomes "Clicking the on-screen toast jumps to the session; a click in the notification center only informs (ADR 0015)."
- Under Do Not Disturb no toast pops up, so the notification is only in the center and a click there does nothing. This is acceptable: the strip's knock animation still shows (§8.4 suppression rule).
- **Phase 2 item:** the dev build's toasts show as "Windows PowerShell" with its icon, because an unpackaged build borrows PowerShell's app identity. The installed build must show "Knock". The NSIS installer's Start menu shortcut must carry the app identity `dev.knock.desktop` (or the final identifier). Check this when the installer exists.
- The V-12 toast code in `toast.rs` is Phase 0 only. The engine's `Notify` effect replaces it in Phase 2.

## Evidence

**Setup:** Windows 11 Pro 10.0.26200.7462, Claude Code 2.1.302, tauri-plugin-notification 2.7.0 (notify-rust 4.18.2, tauri-winrt-notification 0.8.1), debug build a94132d via `pnpm dev`. Classic console session in `%LOCALAPPDATA%\Knock\phase0-project`, default permission mode, with Notepad in front.

**User's manual test on 2026-10-18:**

1. A real `PermissionRequest` (Write) showed the toast "phase0-project needs you / Claude Code is asking for permission to use Write." The header read "Windows PowerShell", with PowerShell's icon.
2. Clicking the toast while it was on screen brought the PowerShell window to the front. The log shows `toast action action="tap"`, then `jump via="toast" precision=ExactWindow front=true flashed=false took_us=4733`. So `SetForegroundWindow` works from a toast click, too.
3. Letting the toast time out, then clicking it in Win+N, did nothing. The log has `toast shown` but no `toast action` line.
4. With Do Not Disturb on, no toast popped up, but the notification was listed in Win+N. Clicking it there did nothing.
