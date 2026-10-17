# 0001: Strip window behavior and click-through

- **Status:** Approved by user on 2026-10-17
- **Date:** 2026-10-17
- **Spec sections:** §6 (Focus, CPU), §9.1, §9.3
- **VERIFY item:** V-10

## Context

V-10 asks whether the strip can be transparent, never focused, hidden from Alt-Tab and the taskbar, stay above the taskbar when the taskbar is clicked, and toggle click-through about 30 times a second without flicker.

Tauri's own click-through call (`set_ignore_cursor_events`, tao 0.37.1) rewrites the whole extended style from tao's internal flags. Each time it does, it drops `WS_EX_TOOLWINDOW`, re-adds `WS_EX_APPWINDOW`, toggles `WS_EX_LAYERED` and forces a frame change. Any other tao window setter has the same effect. The fresh review of d81fc40 confirmed this, citing tao `window_state.rs:430` and `event_loop.rs:1947`.

## Decision

- Create the strip with Tauri's §9.1 options, including `no_redirection_bitmap`.
- Manage the extended styles with Win32 in the platform layer:
  - `WS_EX_NOACTIVATE`, `WS_EX_TOOLWINDOW` and `WS_EX_LAYERED` are always set.
  - `WS_EX_APPWINDOW` is always cleared.
  - Only `WS_EX_TRANSPARENT` toggles, to turn click-through on and off.
- Re-assert these styles on every cursor-poll tick, not only when the hit state changes. A tick writes only when the styles differ, so it stays cheap. This repairs any later rewrite by tao within about 33 ms.
- Never call Tauri's `set_ignore_cursor_events` on the strip. Avoid tao window setters on the strip where a platform-layer call works instead.

## Consequences

- All strip style logic lives in `apps/desktop/src-tauri/src/platform/`. Later phases that touch the strip window must go through it.
- The per-tick re-assert is a follow-up fix to d81fc40; the fresh review flagged it as a latent bug.
- Not covered by V-10: the 154 MB memory reading (dev build). Measure a release build in Phase 3, when the budgets are checked.

## Evidence

**Setup:** Windows 11 Pro 25H2 (build 26200.7462), WebView2 155.0.4283.45, Tauri 2.12.4, debug build. One 1920×1080 monitor at 100% scale, taskbar at the bottom.

**Automated checks by `core-windows`, run from an outside process:**

- **Styles:** at rest, the extended style read back as `0x082801B8`. That is NOACTIVATE, TOOLWINDOW, TOPMOST, LAYERED, TRANSPARENT and NOREDIRECTIONBITMAP, with no APPWINDOW. With the cursor over the target it read `0x08280198`, which is TRANSPARENT cleared. With the cursor off the target it went back to the first value.
- **Click-through:** with the cursor inside the strip but outside the target, `WindowFromPoint` returned the window underneath.
- **Focus:** a synthetic click on the target incremented the page counter, and the foreground window did not change.
- **Taskbar and Alt-Tab:**
  - UI Automation found no Knock button on the taskbar.
  - Checking the window against the usual Alt-Tab rules said it is not eligible.
- **Flicker:** 300 enter and leave cycles over 16 s, sampled 900 times. Every sample showed either the normal or the hover color, never blank, white or black. The transparent area always showed the desktop behind it.
- **CPU at idle:** 0.046% of the machine (about 0.55% of one core) over 60 s, debug build.

**Manual checks by the user on 2026-10-17 (same machine):**

1. Clicking the taskbar, then Start, then the clock: the square stayed visible. **Pass.**
2. Alt-Tab and the taskbar: Knock does not appear in either. **Pass.**
3. Clicking the square while typing in another app: the counter went up and the typing kept going to the other app. **Pass.**
4. Moving the mouse back and forth over the square for about 10 s: it turned green on hover with no flicker. **Pass.**
5. Clicking beside the square: the click reached the window behind it. **Pass.**
