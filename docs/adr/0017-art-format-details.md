# 0017: Art format details left open by §8.7

- **Status:** Approved by user on 2026-10-29
- **Date:** 2026-10-29
- **Spec sections:** §8.7
- **VERIFY item:** none

## Context

§8.7 names the rig and clip contents: keyframe tracks `t`, `x`, `y`, `rot`, `scaleX`, `scaleY`, `alpha`, relative to the rig pose, plus part swaps. It doesn't fix the field names for tracks and swaps, the units, the easing, part visibility at rest, or how play-once clips chain. The `ui` agent picked the smallest workable answers and wrote them in `art/README.md`. §8.7 is a §8 contract, so they're recorded here.

## Decision

The details in `art/README.md` (Parts and Clips sections) become part of the §8.7 contract. In short:

- **Parts:**
  - Every part SVG uses `viewBox="0 0 64 64"` and is drawn in place.
  - `pivot` is in viewBox units.
  - Parts have no parent/child link.
  - Flat shapes only, with at most 6 colors counted over `fill`, `stroke` and `stop-color`.
- **Rig pose:**
  - Every part is shown except `eyes_closed`.
  - The `card` sits behind the body until `present` moves it out.
- **Clip shape:** `{ name, loop, durationMs, tracks: { <part>: [keyframes] }, swaps: [{ from, to, start, end }] }`, and both `tracks` and `swaps` are always present.
- **Units:**
  - `t` is in ms from the start of the clip.
  - `x` and `y` are viewBox units, with up negative.
  - `rot` is in degrees, clockwise positive.
  - `scaleX`, `scaleY` and `alpha` are multipliers.
  - A field a track doesn't set stays at the rig pose.
- **Easing:** sine ease-in-out between the keyframes that set a field, holding before the first and after the last.
- **Swaps:** active while `start <= t < end`.
- **Play-once and chaining:**
  - A play-once clip holds its last frame.
  - Chaining (`present` → `rest`, `step_up` → `knock` → `step_down`) belongs to the character controller, not the clip file.
  - `rest` starts where `present` ends.
  - `step_up` ends 12 units up, and the controller keeps that offset while `knock` plays.

## Consequences

- SPEC.md §8.7: add "Field names, units, easing and chaining: `art/README.md` (ADR 0017)."
- `pnpm art:check` enforces these rules (`apps/desktop/ui/gallery/artCheck.ts`). It also requires a `viewBox` of exactly `0 0 64 64` and flat shapes only, which §8.7 doesn't ask for.
- The Phase 3 PixiJS renderer and character controller must follow this. Adding a part hierarchy later would be a §8.7 change with its own ADR.

## Evidence

none
