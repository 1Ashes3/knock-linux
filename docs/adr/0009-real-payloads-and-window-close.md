# 0009: Real hook payloads (V-3) and closing the terminal window (V-5)

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §8.3, §8.4, §11.3, §11.4
- **VERIFY item:** V-3, V-5

## Context

V-3 asks which payload fields Claude Code really sends on Windows and which tool names appear. V-5 asks what happens when the user closes the terminal window. The evidence comes from Spike B's automated runs and from the user's interactive test on 2026-10-17.

## Decision

- **V-3:**
  - The §8.3 mapping fields exist as named: `session_id`, `cwd`, `hook_event_name`, `tool_name`, `notification_type`, plus `error` on `StopFailure` (ADR 0007).
  - `source` is unavailable (ADR 0005).
  - The adapter reads fields from an allow-list. It keeps `session_id`, `cwd` (for `project` and the cwd hash), `hook_event_name`, `tool_name`, `notification_type`, `error` and `agent_id`. It drops everything else at parse time, including every path field (`transcript_path`, `agent_transcript_path`, `scratchpad_dir`) and every free-text field found:
    - `prompt`
    - `tool_input` and `tool_response`
    - `message`
    - `last_assistant_message`
    - `custom_instructions`
- **V-5:** closing the terminal window sends `SessionEnd` with `reason: "other"`. That maps to `session.ended` as §8.3 already says. Process liveness (§8.4, `ended_unexpectedly`) stays as the fallback for when it doesn't arrive.

## Consequences

- SPEC.md §8.3: the free-text sentence becomes "The adapter reads only the allow-listed fields (ADR 0009) and drops everything else at parse time."
- §11.4 is unchanged. `agent_id` is used only for the same-agent rule in ADR 0010 and is not stored beyond the open episode.
- State-model questions raised by this evidence are decided in ADR 0010.

## Evidence

**Setup:** Windows 11 Pro 25H2 (10.0.26200.7462). Claude Code 2.1.301 for the automated runs; 2.1.302 is installed now, and no payload carries a version. Spike B debug server with the Phase 0 recorder (ADR 0008).

**How the evidence was read:** the user ran one interactive `claude` session in Windows Terminal. A script in the scratch folder read only field names, types and allow-listed enum values. Free-text values were never printed.

**Fields on every event:**
- `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `prompt_id` and `scratchpad_dir`, all strings.
- `cwd` matched the project folder.
- One `session_id` covered the whole session, across a Knock restart and `/compact`.

**Fields on some events:**
- `permission_mode` (`"auto"`, `"default"`)
- `effort.level`

**Fields by event:**

| Event | Fields |
| --- | --- |
| UserPromptSubmit | `prompt` (free text) |
| PreToolUse | `tool_name`, `tool_input`, `tool_use_id`, and `agent_id` when a background agent sends it |
| PostToolUse | the PreToolUse fields, plus `tool_response` and `duration_ms` |
| PermissionRequest | `tool_name`, `tool_input`; no `tool_use_id` |
| Notification | `notification_type`, `message` (free text) |
| Stop | `last_assistant_message`, `stop_hook_active`, `background_tasks`, `session_crons` |
| SubagentStop | `agent_id`, `agent_type` (`""` in all 6 cases), `agent_transcript_path`, and sometimes `last_assistant_message` |
| PreCompact | `trigger` (`"manual"`), `custom_instructions` |
| SessionEnd | `reason` |
| StopFailure | `error` (a code), `last_assistant_message` |

**Tool names seen:** PowerShell, Bash, Read, Glob, Agent and AskUserQuestion.

**Notification types seen:**
- `permission_prompt`: about 6.0 s after `PermissionRequest`, in 6 of 6 pairs that had one.
- `idle_prompt`: 60.0 s after `Stop`, once, with no repeat for 2 minutes.
- `elicitation_dialog`: never seen. The question tool came in as PreToolUse, then PermissionRequest (`tool_name: "AskUserQuestion"`), then `permission_prompt`, then PostToolUse.

**Other observations:**
- `PreCompact` fired on `/compact`. No `Stop` followed it.
- In 5 of 6 turns, a `SubagentStop` with an empty `agent_type` arrived 1.8 to 5.1 s after `Stop`. One turn also sent a `PreToolUse` with `agent_id` after `Stop`. `SubagentStart` never fired.

**V-5:** after the user closed the window with its X button, `SessionEnd` arrived with `reason: "other"`, 16 s after the last event. No other event came with it. The window-close moment wasn't recorded separately, so this is matched by the user's step order.

**Not settled by this test:** two of the test steps produced no permission prompt (the session was in `auto` mode). A retest covers this. See ADR 0010.
