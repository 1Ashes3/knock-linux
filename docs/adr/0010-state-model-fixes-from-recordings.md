# 0010: State-model fixes from real recordings

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §8.2, §8.3, §8.4
- **VERIFY item:** V-3 (follow-up)

## Context

The recordings behind ADR 0009 showed four cases that §8.4 as written would handle wrongly:

1. The `permission_prompt` Notification arrives about 6 s after `PermissionRequest`. §8.4 treats the two as one episode only if they are within 2 s, so the user would get a second knock.
2. Background agents send events after `Stop`: a `SubagentStop`, and sometimes a `PreToolUse` carrying `agent_id`. `PreCompact` can also arrive while a session is Ready. All of these map to `activity`. §8.4 has no `ready + activity` row, so the session would flip back to Working. And a background agent's activity would close a Needs-you episode that the main agent opened.
3. The question tool arrives as `PermissionRequest` with `tool_name: "AskUserQuestion"`, not as an `elicitation_dialog` Notification. The chip would read "Needs permission: AskUserQuestion".
4. Claude Code accepts a settings file that starts with a UTF-8 BOM. §8.2 setup would reject that file as invalid JSON.

## Decision

1. **Dedup window:** a `PermissionRequest` and a `permission_prompt` Notification for the same session within **10 s** are one episode with one `Notify`. This holds even if the episode closed in between.
2. **Background activity:**
   - In `ready` or `idle`, `activity` changes nothing and emits no effects.
   - An `activity` closes a `needs_input` episode only if it comes from the same agent that opened it: the same `agent_id`, or no `agent_id` on either.
   - The normalized event carries `context.agentId` when the payload has `agent_id`.
3. **Questions:** `PreToolUse`/`PermissionRequest` with `tool_name: "AskUserQuestion"` maps to `needs.answer`. The `needs.kind` of an episode is set by the event that opened it, and later events in the same episode don't change it.
4. **BOM:** setup ignores a leading UTF-8 BOM when it reads the settings file, and writes the file without one.

## Consequences

SPEC.md edits:

- §8.2 setup step 1: "A leading UTF-8 BOM is ignored."
- §8.2 step 5: "written without a BOM".
- §8.3:
  - Add `agentId` (optional) to `context`.
  - Add the AskUserQuestion mapping row.
- §8.4:
  - Add a row: `ready`, `idle` + `activity` → unchanged, no effects.
  - The `needs_input` + `activity` row gets "(same agent only)".
  - Dedup window changes from 2 s to 10 s.
  - Add the rule that `needs.kind` stays fixed for the episode.

Phase 1 engine and adapter tests must cover each of these rules (§12.1).

## Evidence

See ADR 0009 for the earlier recordings.

**Retest on 2026-10-18:** one interactive session, `permission_mode` "default" throughout.

- **Step 1, approved fast:**
  - `PermissionRequest` came 327 ms after `PreToolUse`.
  - `PostToolUse` came 1371 ms after that.
  - No `permission_prompt` Notification arrived at any point in the session. A fast approval never produces the late Notification, so the episode relies on `PermissionRequest` alone.
- **Step 2, approved slowly:**
  - The `permission_prompt` Notification arrived 6016 ms after `PermissionRequest`.
  - `PostToolUse` arrived about 22 s after `PermissionRequest`.
- **After each `Stop`:** a `SubagentStop` with an empty `agent_type` arrived 1.6 s and 2.4 s later. Decision 2 covers this.
- **Window close:** one `SessionEnd` arrived, with reason "other".

Across every recording, the gaps between `PermissionRequest` and the Notification were 5996, 6002, 6002, 6005, 6007, 6016 and 6021 ms. A 10 s window covers all of them with margin.
