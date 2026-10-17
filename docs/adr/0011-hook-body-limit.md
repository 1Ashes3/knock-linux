# 0011: Hook body limit 16 MB, never an error reply

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §6 (Agent impact, Reliability), §8.1
- **VERIFY item:** none

## Context

`PermissionRequest` and `PreToolUse` payloads include `tool_input`. A Write of a large file can push the body past §8.1's 256 KB limit. The server then replied `413` and dropped a Needs-you event. That works against §6 "zero missed Needs-you". It was also untested how Claude Code reacts to a `413`. This was finding 1 of the fresh review of cdec79b.

## Decision

- The body limit is 16 MB.
- A body over the limit still gets `200 {}`. It is dropped and logged as a warning without the body.
- `POST /v1/hooks/claude-code` with a valid token never gets an error status. That's the only request Claude Code makes. Other paths and methods may still get `404` or `405`. The wording was narrowed on 2026-10-18 after the second fresh review; it matches what the user approved: "never send Claude Code an error".
- Over the limit, the server reads and discards the rest of the body before replying, so the client's upload is never reset.

## Consequences

- SPEC.md §8.1: "body limit 256 KB" becomes "body limit 16 MB; over the limit: reply `200 {}`, drop, log a warning without the body (ADR 0011)".
- Up to 16 MB per request is read into memory. The server only accepts loopback connections with a valid token.
- If a payload is over 16 MB, its event is still missed. The `permission_prompt` Notification about 6 s later (ADR 0010) then opens the episode instead.

## Evidence

none
