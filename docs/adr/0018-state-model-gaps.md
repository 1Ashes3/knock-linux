# 0018: Filling gaps in the §8.4 transitions

- **Status:** Proposed
- **Date:** 2026-10-29
- **Spec sections:** §8.4
- **VERIFY item:** none

## Context

Building the Phase 1 engine showed (state, input) pairs that §8.4 doesn't list. For those, the engine currently leaves the session unchanged. Some of them would miss a Needs-you, or keep a wrong state. ADR 0016 can also be read in two ways when a new request arrives while a seen episode is still open.

## Decision

1. **New request while a seen episode is open.** A `PermissionRequest` or a question (`needs.answer`) from Claude Code, that is not the deduplicated `permission_prompt` Notification, opens a new, unseen episode with its own `Notify`. While the episode is still unseen, the §8.4 row stands: same episode, no effects.
2. **`stalled` + `needs.permission` or `needs.answer`:** goes to `needs_input`, as a new episode with `Notify`. `stalled` joins the `working, idle, ready` row.
3. **`stalled` + `turn.started`:** goes to `working`.
4. **`failed` + `turn.started` or `activity`:** goes to `working`. **`failed` + `needs.*`:** goes to `needs_input`, as a new episode with `Notify`. The user retried, so Failed is over. Dismiss still removes it.
5. **`ended_unexpectedly` + any agent event:** the session continues, in the state that event implies, as if it were the session's first event. This covers `--resume` in a new terminal.
6. **Suppression drops, it does not defer.** A `Notify` or `Escalate` that falls during pause or quiet hours is not sent later. The strip still shows the state.
7. **No known process:** a session whose process was never found counts as having no live process. It is removed 6 h after its last event if `SessionEnd` never arrives.
8. **Every other pair §8.4 doesn't list** leaves the session unchanged. Examples: `idle` + `turn.completed`, `working` + `turn.idle`, `user.ack` on `working`.

## Consequences

- SPEC.md §8.4:
  - Add `stalled` to the needs row.
  - Add the rows for items 1 to 5.
  - Add a sentence each for items 6, 7 and 8.
- `core-windows` updates the engine and adds a test for each item.
- The expected files only change if a fixture hits one of these pairs. None do today.

## Evidence

none. The cases come from the Phase 1 engine review.
