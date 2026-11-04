# 0031: Calm animals settle, then stay still

- **Status:** Approved by user on 2026-11-04
- **Date:** 2026-11-04
- **Spec sections:** §1, §6 (CPU), §8.7 (required clips), §10.1
- **VERIFY item:** none

## Context

§1 and §10.1 say only a session that needs the user moves: "Nothing moves except on a state change, or while in a looping Needs-you clip." The §8.7 clip table marks `sleep`, `work`, `wait` and `rest` as looping, though, and §6 expects the render loop to stop when nothing animates. The spec doesn't say how long a calm animal moves after a state change.

## Decision

The user chose "settle, then still" on 2026-11-04:

- Calm tiers (Working, Ready, Resting): on a state change the animal plays one cycle of its clip, then holds that clip's first frame. `present` plays once and is followed by one cycle of `rest`, which then holds.
- Needs-you tier: `knock`, `pace` and `alert` keep looping until the state changes. A seen `needs_input` (ADR 0016) holds a calm pose, as §10.1 already says.
- The sleep "z" is shown but stops moving once its animal has settled.
- When no animal is moving, the render loop stops.

The `loop` flag in a clip file still describes the clip. It doesn't decide how long the overlay plays it.

## Consequences

- SPEC.md §10.1 gets a bullet: "A calm animal (Working, Ready, Resting) plays one cycle of its clip on a state change, then holds still. Only Needs-you clips keep looping (ADR 0031)."
- The gallery still loops every looping clip, for review.
- Once every animal on screen has settled, the idle CPU budget applies (under 1%).

## Evidence

none
