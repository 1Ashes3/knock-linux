# 0040: Introduce sprites demo

- **Status:** Approved by user on 2026-11-10 (with the safety stop)
- **Date:** 2026-11-10
- **Spec sections:** §9.7 (tray menu), §10.1 (states), §8.4, §8.5
- **VERIFY item:** none

## Context

Users with a sprite pack have no quick way to see all its characters and how they move. The user asked for a tray-menu button that shows the selected pack's sprites and runs a short demo.

## Decision

Add a tray item "Introduce sprites". When clicked:

1. The strip shows every character of the selected pack, asleep, as demo sprites (the "introduction").
2. Four random characters then each go through sleeping, working, needs-you (knock), failed, and done, a few seconds per state.
3. The demo ends and the strip returns to the real sessions.

The user chose that the demo **replaces** the strip while it runs (real sprites hidden, not removed). To protect Knock's main job, the demo stops at once, and the real strip returns, if a real session starts needing the user. Real notifications and the engine keep running throughout; demo sprites never produce notifications, a jump target or a tray count.

The demo is local to the overlay and driven by the app. It does not go through the hook server, so it needs no session ids from agents and never touches §8.1 or §8.2.

## Amendment (2026-11-10, user request)

- While the demo runs, the core forces the strip on top of all windows, even with placement "desktop", and shows it even with placement "hidden", then restores the setting. The event carries `{ durationMs }` (18 000); the core restores after that long. A safety stop ends the demo early but the strip stays raised until the time is up, which also keeps the real knock visible.
- The picked characters no longer share one state. Each shows a different state at the same time (sleeping, working, knocking, failed, done), and the states rotate between the characters every few seconds so each one passes through all of them.

## Consequences

- §9.7 Tray menu row gains "Introduce sprites".
- New core command and event (§8.5) to start and stop the demo; the overlay draws demo sprites from the active pack, using the usual clips.
- Packs with many characters may need the strip to scroll or shrink; the UI brief decides how within §10.
- Needs a manual test checklist (§12.2).

## Evidence

none
