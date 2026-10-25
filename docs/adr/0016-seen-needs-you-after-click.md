# 0016: Clicking a Needs-you character marks it seen

- **Status:** Approved by user on 2026-10-25
- **Date:** 2026-10-25
- **Spec sections:** §8.4, §8.5, §10.1, §10.2
- **VERIFY item:** none

## Context

The fixtures (`permission-denied`, `interrupt`, `deny-then-wait`, `esc-then-wait`) show that Claude Code sends no hook when the user denies a permission or dismisses it with Esc. Nothing arrived for more than 3 minutes, and no `idle_prompt` either. A permission that is still pending looks exactly the same (`closed-while-waiting`). Under §8.4, Knock would keep knocking after a deny, and after 5 minutes it would send a false escalation.

## Decision

- `user.ack` on a `needs_input` session keeps the state `needs_input` and marks the episode **seen**. `jump_to_session` already sends `user.ack`, so clicking the character or the on-screen toast does this.
- A seen episode:
  - plays no knock animation
  - never sends `Escalate`
  - shows the chip "Waiting for you"
- The episode closes as before: on activity from the same agent, `turn.started`, `turn.completed` or `session.ended`.
- A new `needs.*` event for the same session after the dedup window opens a new, unseen episode, with its own `Notify` (§8.4).

## Consequences

SPEC.md edits:

- **§8.4 transitions:** add the row `needs_input` + `user.ack` → `needs_input` (seen); effects: none, and no `Escalate` for this episode.
- **§8.4 escalation row:** add "not seen".
- **§8.4 invariants:** add "A seen episode emits no `Escalate`."
- **§8.5 `SessionView.needs`:** add `seen: boolean`.
- **§10.1:** add the row "`needs_input` (seen) | calm pose, no knock | 'Waiting for you' | none".

Trade-off the user accepted: if the user clicks the character but then doesn't answer in the terminal, Knock won't remind them again.

## Evidence

**Setup:** Windows 11 Pro 10.0.26200.7462, Claude Code 2.1.30x, recording on 2026-10-25. These are now the fixtures `deny-then-wait` and `esc-then-wait`.

- **Deny:** `PermissionRequest(Write)` at 3.2 s, then nothing until `SessionEnd` at 221 s.
- **Esc:** `PermissionRequest(Write)` at 2.9 s, then nothing until `SessionEnd` at 211 s.
- Neither run sent an `idle_prompt` Notification. Neither sent the `permission_prompt` Notification either, because both answers came within 6 s.
