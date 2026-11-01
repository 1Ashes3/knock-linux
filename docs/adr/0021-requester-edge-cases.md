# 0021: Two edge cases in ADR 0019

- **Status:** Approved by user on 2026-11-01
- **Date:** 2026-11-01
- **Spec sections:** §8.4
- **VERIFY item:** none

## Context

Implementing ADR 0019 (7b25c98) left two edge cases:

1. **A late reminder that doesn't say who it is for.** It isn't recorded whether a sub-agent's late `permission_prompt` Notification carries `agent_id`. If it doesn't, per-requester dedup counts it as the main agent's. Example: a sub-agent's request is answered within 10 s, and then its late prompt arrives. It is no longer a duplicate, so it opens a new episode. That is a false knock, never a lost one.
2. **Answering a background request after the main agent has finished.** If a sub-agent asks after the main agent's `Stop`, and the user answers, the episode closes to `working`. If no more main-agent events come, the session stalls 10 minutes later and shows "No progress" (Needs you), which is wrong.

## Decision

1. A `permission_prompt` Notification with no `agent_id` is a duplicate if any requester of the session made a request in the last 10 s.
2. If the main agent's `turn.completed` was absorbed while sub-agents were waiting, the session goes to `ready` with `ReadyBatchAdd` once the last requester is answered, not to `working`.

## Consequences

- SPEC.md §8.4:
  - In the Deduplication sentence, add "a prompt without `agentId` matches any requester's request in the last 10 s (ADR 0021)".
  - In the `needs_input` + `activity` row, add "goes to `ready` + `ReadyBatchAdd` instead when the main agent's turn already completed (ADR 0021)".
- `core-windows` adds a test for each case.
- Recording a slow sub-agent permission while dogfooding would show which `agent_id` the prompt really carries.

## Evidence

none. Both cases come from the 7b25c98 report.
