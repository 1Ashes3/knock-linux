# 0007: StopFailure is the source of `agent.failed`

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §8.2, §8.3, §8.4
- **VERIFY item:** V-4

## Context

§8.3 leaves the source of `agent.failed` open (V-4). Spike B found that Claude Code fires `StopFailure` over HTTP when a turn ends because of an API error. Its `error` field is a short code, and its `last_assistant_message` field is free text.

## Decision

- Register `StopFailure` in §8.2, with no matcher.
- Map it to `agent.failed` with `reason` set to the `error` code.
- `last_assistant_message` is dropped at parse time (§8.3, §11.3).
- `PostToolUseFailure` is not registered. A failed tool is normal, Claude Code carries on, and `PreToolUse` already counts as activity.

## Consequences

- SPEC.md:
  - §8.2: add `StopFailure` to the event list.
  - §8.3: add a mapping row "`StopFailure` → `error` → `agent.failed`".
  - §8.3: the `agent.failed` kind row reads "(source: `StopFailure`, ADR 0007)".
  - §8.4: `failed` is now "Yes".
- Not tested: rate limits and crashes. If a rate limit or a crash never fires `StopFailure`, those cases stay undetected. The fallback is `ended_unexpectedly` via process liveness (§8.4). Check the rate-limit case when it happens naturally while dogfooding.

## Evidence

Windows 11 Pro 25H2 (build 26200.7462), Claude Code 2.1.301, Spike B server.

`claude -p --model claude-nonexistent-model-1` sent `UserPromptSubmit`, then `StopFailure` (`error: "model_not_found"`, a string code), then `SessionEnd`. It sent no `Stop`, and the run exited with code 1.

A Read of a missing file sent `PostToolUseFailure` instead of `PostToolUse`.
