# 0014: Re-check a session's process on every prompt

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §9.4
- **VERIFY item:** none

## Context

§9.4 looks up a session's process only on its first request. If the same session id later arrives from another process, the jump keeps pointing at the old window and a click does nothing. That could happen with `claude --resume` in a new terminal while Knock is running. This was finding 3 of the Spike C fresh review.

## Decision

Repeat the §9.4 connection-owner lookup on every `UserPromptSubmit`, as well as on a session's first request. If the owner changed, re-run the parent walk and replace the stored process, owner and precision.

## Consequences

- SPEC.md §9.4 step 1: "On the first request for a session" becomes "On the first request for a session, and on every `UserPromptSubmit` (ADR 0014)".
- Each prompt costs about 0.2-0.5 ms more for the lookup, well within §6's 100 ms. The window walk runs only when the owner changed.

## Evidence

none. Lookup timings are in ADR 0012.
