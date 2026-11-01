# 0019: Several pending requests in one session; background activity can't clear Failed

- **Status:** Approved by user on 2026-11-01
- **Date:** 2026-10-29
- **Spec sections:** §8.4
- **VERIFY item:** none

## Context

The fresh review of b66c13f and aea12c3 found two ways §8.4 (with ADRs 0010 and 0018) can lose a Needs-you, and one way it can lose a Failed state:

1. **Two agents ask at once.** Sub-agent A asks for permission, then sub-agent B does. §8.4 folds B into A's open episode and remembers only A. When A's request is approved, A's activity closes the episode while B is still waiting. B's `permission_prompt` Notification then counts as a duplicate, so B gets no knock and no escalation.
2. **A background agent asks, then the main agent finishes.** The main agent's `Stop` moves `needs_input` to `ready`, and the background agent's waiting request is lost in the same way.
3. **Background activity clears Failed.** ADR 0018 item 4 lets any `activity` clear `failed`. A background agent's `SubagentStop` arrives 1.6-5 s after a turn ends (ADRs 0009 and 0010). If one follows a `StopFailure`, Failed silently becomes Working, and later Stalled.

## Decision

1. **An episode tracks every waiting requester.** A requester is the agent that asked: the main agent (no `agent_id`) or a sub-agent (its `agent_id`).
   - `needs.*` from a new requester while `needs_input` is open adds that requester to the same episode. It sends no new `Notify`, because the knock is already showing. If the episode was seen (ADR 0016), it opens a new, unseen episode with `Notify`, as ADR 0018 item 1 already says.
   - `activity` from a requester removes that requester only. The episode closes, and the session goes to `working`, when no requester is left.
   - `turn.completed` (the main agent's `Stop`) removes the main agent only. If sub-agents are still waiting, the session stays `needs_input`. Otherwise it goes to `ready` with `ReadyBatchAdd`, as today.
   - `turn.started` (the user typed) still closes the whole episode.
2. **Only the main agent clears Failed.** `failed` + `activity` goes to `working` only when the activity has no `agent_id`. `turn.started` still clears it (the user retried). Background activity leaves Failed as it is.
3. Dedup (ADR 0010) works per requester.

## Consequences

SPEC.md §8.4 edits:

- The `needs_input` + `needs.*` row reads "same episode; the requester is added; a seen episode opens a new unseen one with `Notify`".
- The `needs_input` + `activity` row reads "removes that requester; closes when none is left".
- The `needs_input` + `turn.completed` row reads "removes the main agent; `ready` + `ReadyBatchAdd` only when none is left".
- The `failed` + `activity` row (ADR 0018) gets "(main agent only)".

`core-windows` updates the engine, with tests for the reviewer's two scenarios and the background-SubagentStop-after-StopFailure case. It also extends `knock replay`'s missed-Needs-you check to see a new episode that replaces a seen one.

## Evidence

The fresh review reproduced findings 1 and 2 with knock-core in a throwaway project. Finding 3 follows from the recorded post-`Stop` `SubagentStop` timings in ADRs 0009 and 0010. It isn't recorded after a `StopFailure` yet.
