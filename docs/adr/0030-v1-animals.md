# 0030: The v1 animals are fox, rabbit, cat and owl

- **Status:** Approved by user on 2026-11-04
- **Date:** 2026-11-04
- **Spec sections:** §8.7 (v1 animals), §8.4 (assign animal)
- **VERIFY item:** none

## Context

§8.7 says the four v1 animals are "chosen during Phase 3". The fox was drawn in Phase 1 and approved. Three more were needed. They have to be easy to tell apart at 48 px and must not look like any known character or mascot.

## Decision

The user chose rabbit, cat and owl on 2026-11-04. The animal ids are `fox`, `rabbit`, `cat` and `owl`, and the engine assigns them in that order.

## Consequences

- SPEC.md §8.7 "v1 animals" becomes: "fox, rabbit, cat and owl, assigned in that order (ADR 0030)."
- The UI draws `art/animals/rabbit`, `art/animals/cat` and `art/animals/owl` with every required part (§8.7). The owl's ear tufts are its `ear_l` and `ear_r`.
- `core-windows` changes the engine's animal list from `["fox"]` to the four ids. That lands only after all four animals pass `pnpm art:check`.

## Evidence

none
