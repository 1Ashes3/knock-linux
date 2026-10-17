# 0002: Strip placement with an auto-hidden taskbar

- **Status:** Approved by user on 2026-10-17
- **Date:** 2026-10-17
- **Spec sections:** §9.2
- **VERIFY item:** V-13

## Context

§9.2 says that when the taskbar auto-hides, the strip must stay above the reveal zone. V-13 asks how big that zone is and where the strip should sit.

## Decision

When the taskbar auto-hides, the strip's bottom edge sits on the top edge of the shown taskbar. That position comes from `ABM_GETTASKBARPOS`, which reports the shown position even while the taskbar is hidden. The other §9.2 rules stay the same: right edge 8 logical px in, grows leftward.

## Consequences

- The strip never overlaps the taskbar or its reveal zone, so it never blocks the taskbar from appearing. We no longer need to know the exact size of the reveal zone.
- While the taskbar is hidden, the strip floats one taskbar height above the bottom of the screen (48 px here).
- SPEC.md §9.2: replace "keep the strip above the reveal zone (**VERIFY V-13**)" with "rest the strip on the top edge of the shown taskbar (ADR 0002)".
- `core-windows` implements this in Phase 2 together with the rest of §9.2. Spike A doesn't need to change.
- Rejected alternative: sit just above the reveal zone, a few pixels from the bottom. The strip would sit lower, but the taskbar would cover it every time it appeared, and we would have to measure the reveal zone on every setup.

## Evidence

**Setup:** Windows 11 Pro 25H2 (build 26200.7462), Spike A build d81fc40. One 1920×1080 monitor at 100% scale, taskbar at the bottom, auto-hide turned on by the user on 2026-10-17.

**App log:**

- Monitor `(0,0)-(1920,1080)`.
- Work area `(0,0)-(1920,1080)`: with auto-hide on, the work area is the whole monitor.
- Taskbar `(0,1032)-(1920,1080)`, edge bottom, auto-hide `true`. This is the shown position, reported while the taskbar was hidden.
- Strip `(1720,1016)-(1912,1080)`, placed on the work area as in Spike A.

**What the user saw:**

- The test square sat just above the bottom edge of the screen. The strip itself reached the bottom edge, and the square has 8 px of padding.
- When the mouse reached the bottom edge, the taskbar slid up and covered the square.
- Hovering the square did not make the taskbar appear, so the strip blocks the reveal zone wherever its hit area is.

**Not measured:** the exact height of the reveal zone. We didn't capture the hidden taskbar's rectangle. The decision above doesn't depend on it.
