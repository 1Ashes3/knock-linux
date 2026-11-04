# Knock — Specification

Version 1.0 · 2026-10-16 · Scope: Stage A (Claude Code CLI on Windows 11)

This file is the single source of truth for building Knock. It is derived from the *Knock: Technical Design Document*. If this file and any other document disagree, this file wins.

**Changing this file:** any change to a contract in §8, to scope (§2) or to a fixed decision (§3) requires an ADR in `docs/adr/` approved by the user. Other clarifications can be made by the lead and noted in `docs/PROGRESS.md`.

**Keywords:**

- **MUST** — required.
- **SHOULD** — expected unless an ADR says otherwise.
- **VERIFY** — an assumption that Phase 0 must confirm or replace (§15). Never treat a VERIFY item as fact; report what you find.

---

## 1. Product summary

Knock is a Windows 11 overlay that shows each running Claude Code CLI session as an animal in a small strip resting on top of the taskbar, near the clock. Only a session that needs the user moves: its animal steps up and knocks on the glass, and Windows shows one notification. Everything else stays calm.

- Open source under the MIT license, in a public repository. No backend, no revenue goal.
- Built entirely with AI: Claude Code writes all code and all art. The user makes decisions and does manual testing; the user does not read code.
- Usable by anyone who runs Claude Code: one installer, guided setup, no command line, no file editing, plain language everywhere.

The four visible **tiers** are the core idea:

| Tier      | Meaning                                                  | Interrupts the user?                                         |
| --------- | -------------------------------------------------------- | ------------------------------------------------------------ |
| Needs you | A session is blocked on the user, failed, or looks stuck | Yes (Needs input and Failed always; Stalled only if enabled) |
| Ready     | A session finished its turn and has output to review     | No; batched                                                  |
| Working   | A session is making progress                             | No                                                           |
| Resting   | Idle, paused, or ended                                   | No                                                           |

Animals are a skin over this status model. Every state MUST also be readable as plain text.

## 2. Scope

**In scope (Stage A):**

- Windows 11 only.
- Claude Code CLI running natively on Windows (PowerShell, Command Prompt or Git Bash), in any terminal or editor.

**Out of scope — do not build, do not add code paths for:**

- WSL2 sessions, the Claude Code panel inside VS Code, the Claude desktop app.
- macOS and Linux (Stage B, deferred) and other agent platforms (Stage C, deferred). Keep the adapter interface (§7.3) and the platform layer interface (§9) clean so these can be added later, but write no implementations for them.
- Approving, denying or answering anything on the agent's behalf.
- Showing or storing agent messages, prompts, tool inputs, transcripts or file contents.
- Logs viewers, usage meters, launching or orchestrating agents, team features, cloud services, telemetry, mobile apps.
- Installer code signing (deferred), winget, Microsoft Store.

## 3. Fixed decisions

| Topic             | Decision                                                                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Name              | Knock                                                                                                                        |
| Operating system  | Windows 11 only                                                                                                              |
| Agent             | Claude Code CLI, native Windows                                                                                              |
| Terminals         | Any terminal or editor that runs the CLI                                                                                     |
| Tech stack        | Tauri 2: Rust core, TypeScript UI, WebView2                                                                                  |
| Placement         | A strip resting on top of the taskbar, right-aligned toward the clock. With several monitors, the user chooses which taskbar |
| Hook installation | Knock's setup edits the user's Claude Code settings file, with a preview, a backup and one-click removal                     |
| Art               | Flat vector (SVG) animals drawn by Claude Code, built from named parts so they can be replaced later without code changes    |
| Agent messages    | Never shown, never stored, never logged                                                                                      |
| License           | MIT, public GitHub repository                                                                                                |
| Distribution      | GitHub Releases with in-app auto-update only                                                                                 |
| Installer signing | Unsigned for now                                                                                                             |
| Team              | Lead (main Claude Code session) plus two subagents: `core-windows` and `ui`. The user tests manually (§16)                   |

## 4. Glossary

- **Session** — one running Claude Code CLI process, identified by Claude Code's `session_id`.
- **State** — one of the engine states in §8.4.
- **Tier** — the user-visible grouping of states: Needs you, Ready, Working, Resting.
- **Episode** — one continuous period a session spends in a Needs-you state. Notifications and escalations are counted per episode.
- **Knock** — the step-up-and-knock animation plus the Windows notification when a session enters Needs you.
- **Strip** — the overlay window resting on the taskbar.
- **Den** — the compact group used when more than 5 sessions are active.
- **Hook** — a rule in Claude Code's settings that sends an event to Knock when something happens in a session.
- **Precision** — how exactly Knock can focus a session's window: `exact_window`, `app_window` or `none` (§9.5).

## 5. Functional requirements

Owner: **CW** = `core-windows` agent, **UI** = `ui` agent.

| ID    | Requirement                                                                                        | Acceptance criteria                                                                                                                             | Phase | Owner                             |
| ----- | -------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----- | --------------------------------- |
| FR-1  | Detect Claude Code CLI sessions automatically after the one-time setup                             | A new session appears in the strip within 1 s of its first hook event; no user action beyond setup                                              | 1-2   | CW                                |
| FR-2  | Map Claude Code events to states and tiers exactly as §8.4                                         | Replay suite passes; every state used in v1 is reachable from fixtures                                                                          | 1     | CW                                |
| FR-3  | One character per active session, or a plain status strip                                          | Switching mode in settings takes effect without restarting                                                                                      | 2-3   | UI                                |
| FR-4  | On entering Needs you: knock animation plus exactly one Windows notification per episode           | Verified by replay (notification effects) and by manual test                                                                                    | 2-3   | CW (notification), UI (animation) |
| FR-5  | Escalate once if Needs you is unanswered for N minutes (default 5)                                 | Exactly one extra notification per episode, never more                                                                                          | 4     | CW                                |
| FR-6  | Click a character to bring its terminal or editor window to the front                              | Behaves per the precision table in §9.5; falls back to flashing the taskbar button                                                              | 4     | CW (+ UI click handling)          |
| FR-7  | Hover a character to see project, state label, time in state and, for Needs you, the decision type | Never shows agent message text                                                                                                                  | 3     | UI                                |
| FR-8  | Batch Ready completions; never interrupt for them                                                  | No notification is ever emitted for Ready                                                                                                       | 2     | CW                                |
| FR-9  | "While you were away" summary after the user returns from 10+ idle minutes                         | Card lists sessions that became Ready, Failed or Needs you during the idle period; folds away on click or after 60 s                            | 4     | CW (detection), UI (card)         |
| FR-10 | Collapse to a den above 5 active sessions                                                          | Den shows per-tier counts; only Needs-you sessions step out of it                                                                               | 4     | UI                                |
| FR-11 | Flag Stalled when a Working session emits no events for N minutes (default 10)                     | Stalled shows in the strip; notification only if the setting is on (default off)                                                                | 4     | CW                                |
| FR-12 | Guided setup and a health check                                                                    | Setup finishes in under 2 minutes with no command line or file editing; health check reports hooks installed, server listening, last event time | 2, 5  | CW + UI                           |
| FR-13 | Restore all session states within 2 s after an app restart                                         | Snapshot restored; process liveness re-checked                                                                                                  | 5     | CW                                |
| FR-14 | Simulator that plays scripted scenarios against the running app                                    | `knock sim <scenario>` works for every scenario in §12.3                                                                                        | 1     | CW                                |
| FR-15 | Follow the taskbar                                                                                 | Strip repositions within 1 s of a display, scaling, work-area or auto-hide change                                                               | 2, 4  | CW                                |

## 6. Non-functional budgets

| Area           | Budget                                                                                                                                                         |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Ease of use    | Install plus setup under 2 minutes; no command line, no file editing; every screen in plain language                                                           |
| Latency        | Hook received to frame drawn: p95 under 500 ms. A goal, not a test (ADR 0033)                                                                                                               |
| Agent impact   | Hook request answered in under 100 ms; if Knock is not running the hook fails fast and silently; if Knock is hung, each hook waits at most the 1 s timeout (ADR 0006); no console window ever appears                                |
| CPU            | Under 1% when nothing animates (render loop stopped); under 5% while 5 characters animate at 30 fps. A goal, not a test (ADR 0033)                                                          |
| Memory         | Under 150 MB total, including WebView2, as Task Manager's Memory column shows it. A goal, not a test (ADR 0026)                                               |
| Installer size | Under 25 MB                                                                                                                                                    |
| Focus          | The strip never takes keyboard focus; never appears in Alt-Tab or on the taskbar                                                                               |
| Displays       | Correct at 100%, 150% and 200% scaling and with multiple monitors                                                                                              |
| Privacy        | No network traffic except the update check; no agent messages, prompts, tool inputs or transcripts stored or logged                                            |
| Reliability    | Zero missed Needs-you events across the replay suite                                                                                                           |
| Accessibility  | Windows "Animation effects" setting respected; every state has a text label; color is never the only signal; Narrator reads each character's project and state |

## 7. Architecture

### 7.1 Components and ownership

| Component             | Path                                   | Owner | Responsibility                                                                                                   |
| --------------------- | -------------------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------- |
| Engine and adapters   | `crates/knock-core/`                   | CW    | Normalized events, Claude Code adapter, state engine, timers. Pure: no I/O, no OS calls, no Tauri, no wall clock |
| Helper CLI            | `crates/knock-cli/`                    | CW    | `knock sim`, `knock replay`, `knock doctor`; optional command-hook bridge if Phase 0 requires it                 |
| App shell             | `apps/desktop/src-tauri/`              | CW    | Local server, effect runner, persistence, tray, hook setup, Tauri commands and events                            |
| Platform layer        | `apps/desktop/src-tauri/src/platform/` | CW    | One Rust trait; Windows implementation only. All Windows API calls live here                                     |
| Overlay UI            | `apps/desktop/ui/overlay/`             | UI    | PixiJS renderer, character controllers, chips, tooltips, den, plain strip                                        |
| Settings and setup UI | `apps/desktop/ui/settings/`            | UI    | Setup flow, settings, health check screen                                                                        |
| Art gallery           | `apps/desktop/ui/gallery/`             | UI    | Every animal in every clip, for review                                                                           |
| Art                   | `art/`                                 | UI    | SVG parts, rigs, clips                                                                                           |
| Fixtures              | `fixtures/`                            | CW    | Recorded, sanitized sessions and expected timelines                                                              |
| Landing page          | `site/`                                | UI    | Static site                                                                                                      |
| CI and release config | `.github/`                             | CW    | Workflows                                                                                                        |
| Docs                  | `docs/`, `SPEC.md`, `CLAUDE.md`        | Lead  | Spec, ADRs, progress log                                                                                         |

### 7.2 Data flow

1. Claude Code fires a hook and POSTs its JSON to Knock's local server (§8.1).
2. The server checks the token. On a session's first request it records which process sent it (§9.4).
3. The Claude Code adapter turns the raw payload into normalized events (§8.3).
4. The engine reduces each event (or a 1-second clock tick) into new session states plus effects (§8.4).
5. The effect runner shows notifications, persists the snapshot and pushes changed sessions to the UI (§8.5).
6. The overlay picks each character's animation from its tier and state and plays the transition.

### 7.3 Architectural rules

- The core is the only source of truth. The UI MUST NOT infer state on its own and MUST NOT talk to Claude Code.
- `knock-core` MUST stay pure so every recorded session can be replayed with a virtual clock.
- Every agent integration MUST go through an adapter that provides: id and display name, a parser from raw payload to normalized events, install and uninstall steps, a health check, and a capability list. Stage A has exactly one adapter: `claude-code`.
- Every OS-specific behavior MUST go through the platform layer trait: overlay window behavior, monitor and taskbar geometry, cursor position and click-through, connection owner and process tree, window lookup and focus, process liveness, idle time, launch at sign-in.

## 8. Contracts

### 8.1 Local server

- Binds to `127.0.0.1` only. Never `0.0.0.0`.
- Port: picked at first run from 49152-65535, then fixed. If it cannot bind later, show a plain-language error with a **Repair** button that picks a new port and re-runs hook setup (with preview).
- Token: 32 random bytes, base64url.
- Port and token stored in `%APPDATA%\Knock\server.json`.

| Endpoint                     | Purpose                         | Rules                                                                                                                                                                                                 |
| ---------------------------- | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POST /v1/hooks/claude-code` | Receive Claude Code hook events | Requires header `X-Knock-Token`; body limit 16 MB, and over the limit reply `200 {}`, drop the event and log a warning without the body (ADR 0011); reply `200` with body `{}` within 100 ms, then process asynchronously; never return decision fields; wrong token → `401`, logged without the body |
| `GET /v1/health`             | Health check                    | Token required; returns `{ version, uptimeS, sessionCount, lastEventAt }`                                                                                                                             |
| `GET /v1/debug/snapshot`     | Tests only                      | Debug builds only; returns the full engine snapshot                                                                                                                                                   |

- Requests with header `X-Knock-Sim: 1` come from `knock sim`. They skip process lookup and liveness checks and are marked as simulated in the snapshot.
- **VERIFY V-1, V-2:** HTTP hooks reach localhost on Windows with a literal header value; Claude Code never blocks or shows an error when Knock is down, slow or returns `401`. If literal headers fail, put the token in the path (`/v1/hooks/claude-code/<token>`) and record an ADR.

### 8.2 Claude Code hook configuration (written by setup)

- File: `<CLAUDE_CONFIG_DIR>\settings.json` if that variable is set, else `%USERPROFILE%\.claude\settings.json` (ADR 0022).
- Events registered: `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Notification`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PreCompact`, `SessionEnd`. `SessionStart` is not registered: Claude Code does not support HTTP hooks for it (ADR 0005). `StopFailure` is the `agent.failed` source (ADR 0007).
- Each event gets one HTTP handler. Shape (`timeout` is in seconds, ADR 0006):

```json
{
  "type": "http",
  "url": "http://127.0.0.1:<port>/v1/hooks/claude-code",
  "headers": { "X-Knock-Token": "<token>" },
  "timeout": 1
}
```

- Tool events (`PreToolUse`, `PostToolUse`, `PermissionRequest`) use the match-all matcher `"*"`; the other events use no matcher (ADR 0006).
- **Ownership:** Knock's entries are identified by a URL starting with `http://127.0.0.1:` and containing `/v1/hooks/claude-code`.

**Setup MUST:**

1. Read the file, ignoring a leading UTF-8 BOM (ADR 0010). If it is not valid JSON, stop and explain in plain language; write nothing. A file with more than one hard link is refused the same way. A symbolic link is followed, and its target is edited and shown in the preview (ADR 0024).
2. Show the user a preview of exactly what will be added or changed.
3. Write only after the user confirms.
4. Back up the original to `%APPDATA%\Knock\backups\settings.<timestamp>.json` before every write.
5. Write atomically (temporary file, then replace), without a BOM.
6. Preserve every non-Knock entry and every other key unchanged.
7. Be idempotent: re-running replaces Knock's old entries rather than adding duplicates.

**Removal MUST** delete only Knock's entries, with the same backup and atomic write.

During development, agents MUST NOT modify the user's real settings file. Use a temporary copy in tests; only the running app's setup flow, with the user's confirmation, may touch the real file.

### 8.3 Normalized event (version 1)

```json
{
  "v": 1,
  "agent": "claude-code",
  "sessionId": "string",
  "ts": "2026-10-16T14:02:11.120Z",
  "kind": "needs.permission",
  "detail": { "tool": "PowerShell", "category": "run" },
  "context": { "cwd": "C:\\path\\repo-name", "project": "repo-name", "pid": 12345, "agentId": "string" }
}
```

- `ts` is the server receive time.
- `project` is the last segment of `cwd`, or `home` for the user's home folder (ADR 0020).
- `pid` is present when known (§9.4).
- `agentId` is present when the payload has `agent_id` (a sub-agent or background agent, ADR 0010).

**Kinds:**

| Kind               | Meaning                                                                         | Detail fields                                       |
| ------------------ | ------------------------------------------------------------------------------- | --------------------------------------------------- |
| `session.started`  | A session started, resumed, was cleared or was compacted                        | `source`: `startup`, `resume`, `clear` or `compact` |
| `turn.started`     | The user submitted a prompt                                                     | —                                                   |
| `activity`         | Tool use, sub-agent activity or compaction                                      | `tool`, `category` (optional)                       |
| `needs.permission` | Claude Code is waiting for a permission decision                                | `tool`                                              |
| `needs.answer`     | Claude Code asked the user a question                                           | —                                                   |
| `turn.idle`        | Claude Code is idle, waiting for the next prompt                                | —                                                   |
| `turn.completed`   | Claude Code finished its turn                                                   | —                                                   |
| `agent.failed`     | Claude Code reported an error it will not recover from (source: `StopFailure`, ADR 0007) | `reason` code only, no text                |
| `session.ended`    | The session ended normally                                                      | —                                                   |
| `user.ack`         | Internal: the user clicked, jumped to or marked the session reviewed            | —                                                   |
| `user.dismiss`     | Internal: the user dismissed a Failed or ended session                          | —                                                   |

**Tool categories:**

| Category | Tools                                |
| -------- | ------------------------------------ |
| `read`   | Read, NotebookRead                   |
| `search` | Grep, Glob                           |
| `edit`   | Edit, MultiEdit, Write, NotebookEdit |
| `run`    | Bash, PowerShell                     |
| `web`    | WebFetch, WebSearch                  |
| `other`  | Anything else, including MCP tools   |

**Hook → event mapping** (field names confirmed in ADR 0009):

| Claude Code hook                              | Matcher or field                         | Normalized event   |
| --------------------------------------------- | ---------------------------------------- | ------------------ |
| `SessionStart`                                | `source`                                 | `session.started` (not available over HTTP; no producer in v1, ADR 0005) |
| `UserPromptSubmit`                            | —                                        | `turn.started`     |
| `PreToolUse`, `PermissionRequest`             | `tool_name = AskUserQuestion`            | `needs.answer` (ADR 0010) |
| `PreToolUse`, `PostToolUse`                   | `tool_name` (any other)                  | `activity`         |
| `PermissionRequest`                           | `tool_name`                              | `needs.permission` |
| `Notification`                                | `notification_type = permission_prompt`  | `needs.permission` |
| `Notification`                                | `notification_type = elicitation_dialog` | `needs.answer`     |
| `Notification`                                | `notification_type = idle_prompt`        | `turn.idle`        |
| `Notification`                                | any other type                           | ignored            |
| `Stop`                                        | —                                        | `turn.completed`   |
| `StopFailure`                                 | `error` (code)                           | `agent.failed` (ADR 0007) |
| `SubagentStart`, `SubagentStop`, `PreCompact` | —                                        | `activity`         |
| `SessionEnd`                                  | —                                        | `session.ended`    |

The adapter MUST read only the allow-listed fields (`session_id`, `cwd`, `hook_event_name`, `tool_name`, `notification_type`, `error`, `agent_id`; ADR 0009) and drop everything else at parse time, including `message`, `prompt`, `tool_input`, `tool_response`, `last_assistant_message`, `custom_instructions` and every path field other than `cwd`. Dropped fields never enter the engine, the snapshot, the logs or the fixtures.

### 8.4 State model

**States and tiers** (states marked "unused in v1" stay in the type definitions with no producing event):

| State                | Tier      | Produced in v1?                   |
| -------------------- | --------- | --------------------------------- |
| `needs_input`        | Needs you | Yes                               |
| `failed`             | Needs you | Yes (`StopFailure`, ADR 0007)     |
| `stalled`            | Needs you | Yes (timer)                       |
| `blocked_fixable`    | Needs you | Unused in v1                      |
| `ready`              | Ready     | Yes                               |
| `working`            | Working   | Yes                               |
| `waiting_agent`      | Working   | Unused in v1                      |
| `communicating`      | Working   | Unused in v1                      |
| `idle`               | Resting   | Yes                               |
| `ended_unexpectedly` | Resting   | Yes                               |
| `paused`             | Resting   | Unused in v1                      |
| `cancelled`          | Resting   | Unused in v1                      |

**Transitions:**

| From                           | Input                                                         | To                                       | Effects                          |
| ------------------------------ | ------------------------------------------------------------- | ---------------------------------------- | -------------------------------- |
| (none)                         | `session.started` or any first event                          | `idle` (or the state that event implies) | Assign animal; record process    |
| `idle`, `ready`, `stalled`, `failed` | `turn.started`                                          | `working`                                | — (ADR 0018)                     |
| `failed`                       | `activity` from the main agent (no `agentId`)                 | `working`                                | — (ADR 0018, 0019)               |
| `working`, `stalled`           | `activity`                                                    | `working`                                | Reset stall timer                |
| `working`, `idle`, `ready`, `stalled`, `failed` | `needs.permission`, `needs.answer`               | `needs_input`                            | New episode; `Notify` (ADR 0018) |
| `needs_input` (seen)           | A new request (`PermissionRequest` or question), not the deduplicated notification | `needs_input`  | New unseen episode; `Notify` (ADR 0018) |
| `ended_unexpectedly`           | Any agent event                                               | The state that event implies, as a first event | As for a first event (ADR 0018) |
| `needs_input`                  | `needs.permission`, `needs.answer`                            | `needs_input`                            | Same episode; the requester is added (ADR 0019) |
| `idle`, `ready`                | `activity`                                                    | unchanged                                | None (ADR 0010)                  |
| `needs_input`                  | `activity` from a waiting requester (ADR 0010, 0019)          | `working` once no requester is left, or `ready` + `ReadyBatchAdd` if the main agent's turn already completed (ADR 0021) | That requester is removed; close episode when none is left |
| `needs_input`                  | `turn.started`                                                | `working`                                | Close episode                    |
| `needs_input`                  | `turn.completed`                                              | `ready` once no sub-agent is waiting     | Main agent removed; close episode and `ReadyBatchAdd` when none is left (ADR 0019) |
| `working`, `stalled`           | `turn.completed`                                              | `ready`                                  | `ReadyBatchAdd`                  |
| `ready`                        | `turn.idle`                                                   | `ready`                                  | None                             |
| `working`                      | Tick: no events for `stallMinutes`                            | `stalled`                                | `Notify` only if `notifyOnStall` |
| `needs_input`                  | Tick: episode open for `escalationMinutes`, not yet escalated, not seen | `needs_input`                  | `Escalate` (once per episode)    |
| `needs_input`                  | `user.ack`                                                    | `needs_input` (episode marked seen, ADR 0016) | None; no `Escalate` for this episode |
| any                            | `agent.failed`                                                | `failed`                                 | `Notify`                         |
| any except `failed`            | Process gone without `session.ended`                          | `ended_unexpectedly`                     | None; fades after 60 s           |
| `ready`                        | `user.ack`                                                    | `idle`                                   | —                                |
| `failed`, `ended_unexpectedly` | `user.dismiss`                                                | removed                                  | —                                |
| any                            | `session.ended`                                               | removed                                  | Fade out                         |

**Timers and clock:**

- The app sends `Tick(now)` once per second. The engine never reads the clock itself.
- Defaults: `stallMinutes` 10, `escalationMinutes` 5.
- A session with no events and no live process for 6 hours is removed. A session whose process was never found counts as having no live process (ADR 0018).
- Any (state, input) pair not listed above leaves the session unchanged (ADR 0018).

**Deduplication:** a `PermissionRequest` and a `permission_prompt` notification for the same session within 10 s are one episode, with one `Notify`, even if the episode closed in between (ADR 0010). Dedup is per requester (ADR 0019); a prompt without `agentId` matches any requester's request in the last 10 s (ADR 0021). An episode's `needs.kind` is set by the event that opened it.

**Effects:**

| Effect          | Fields                                                                           |
| --------------- | -------------------------------------------------------------------------------- |
| `Notify`        | `sessionId`, `reason`: `needs_permission`, `needs_answer`, `failed` or `stalled` |
| `Escalate`      | `sessionId`                                                                      |
| `ReadyBatchAdd` | `sessionId`                                                                      |
| `Persist`       | —                                                                                |

**Suppression:** `Notify` and `Escalate` MUST be suppressed while `pauseNotifications` is on or during quiet hours. The animation still plays. Suppressed effects are dropped, not sent later (ADR 0018).

**Invariants** (MUST be covered by property tests):

1. Every session has exactly one state and one tier.
2. A `needs_input` session always has a `needs.kind`.
3. At most one `Notify` and at most one `Escalate` per episode, and no `Escalate` once the episode is seen (ADR 0016).
4. No `Notify` is ever emitted for `ready`.
5. Replaying the same inputs always gives the same outputs.

### 8.5 Core ↔ UI interface (Tauri)

**Events (core → UI):**

| Event              | Payload                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------ |
| `sessions-changed` | `{ rev: number, sessions: SessionView[] }`                                                 |
| `away-summary`     | `{ since: string, items: [{ id, project, outcome: "ready" \| "failed" \| "needs_you" }] }` |
| `settings-changed` | `{ settings: Settings }`                                                                   |

`SessionView`:

```json
{
  "id": "string",
  "project": "repo-name",
  "tier": "needs_you",
  "state": "needs_input",
  "since": "timestamp",
  "activity": "run",
  "needs": { "kind": "permission", "label": "PowerShell", "seen": false },
  "precision": "app_window",
  "animal": "fox",
  "simulated": false
}
```

**Commands (UI → core):**

| Command                                       | Purpose                                                                                 |
| --------------------------------------------- | --------------------------------------------------------------------------------------- |
| `get_sessions()`                              | Initial load                                                                            |
| `jump_to_session(id)`                         | Focus the session's window (§9.6); also sends `user.ack`                                |
| `acknowledge(id)`                             | Mark Ready as reviewed (`user.ack`)                                                     |
| `dismiss(id)`                                 | Dismiss Failed or ended (`user.dismiss`)                                                |
| `dismiss_away_summary()`                      | Close the summary card                                                                  |
| `set_hit_regions(regions)`                    | `regions: [{ x, y, width, height }]`: character rectangles in logical px, relative to the strip; replaces the previous set; empty means fully click-through (§9.3, ADR 0003) |
| `request_strip_size(width, height)`           | UI asks for the size it needs; core positions the window (§9.2)                         |
| `get_settings()`, `update_settings(patch)`    | Read and change settings (§8.6)                                                         |
| `setup_status()`                              | Hooks installed? Server OK (with `serverError`, ADR 0023)? Monitors available?          |
| `repair_server()`                             | Pick a new port and restart the server (§8.1, ADR 0023)                                 |
| `setup_list_monitors()`                       | For the setup step that picks a taskbar                                                 |
| `setup_preview_hooks()`                       | Returns `{ path, before, after }` for the preview screen                                |
| `setup_apply_hooks()`, `setup_remove_hooks()` | Write or remove hooks (§8.2)                                                            |
| `run_health_check()`                          | Returns the checks and their results in plain language                                  |
| `play_test_knock()`                           | Runs a simulated knock so the user sees what it looks like                              |

Names are the initial contract; changing them requires an ADR.

### 8.6 Settings

Stored in `%APPDATA%\Knock\settings.json`:

```json
{
  "version": 1,
  "monitor": "primary",
  "mode": "animals",
  "size": "medium",
  "sounds": false,
  "stallMinutes": 10,
  "escalationMinutes": 5,
  "notifyOnStall": false,
  "pauseNotifications": false,
  "quietHours": { "enabled": false, "start": "22:00", "end": "08:00" },
  "launchAtSignIn": null,
  "animalOverrides": {}
}
```

- `monitor` is `"primary"` or a stable monitor id. Setup asks the user when more than one display is connected.
- `launchAtSignIn` stays `null` until setup asks the user.
- `animalOverrides` maps a project key to an animal id. The project key is a hash of the lower-cased `cwd`.

### 8.7 Art and animation contract

**Layout:**

- `art/animals/<animal-id>/rig.json` plus one SVG per part.
- `art/clips/<clip>.json`, shared by all animals.

**Rig:**

```json
{
  "id": "fox",
  "displayName": "Fox",
  "viewBox": [0, 0, 64, 64],
  "parts": [ { "name": "body", "file": "body.svg", "pivot": [32, 40], "z": 1 } ]
}
```

- **Required parts:** `body`, `head`, `ear_l`, `ear_r`, `eyes_open`, `eyes_closed`, `paw_l`, `paw_r`, `tail`, `card`.
- **Clips** contain `name`, `loop`, `durationMs`, per-part keyframe tracks (`t`, `x`, `y`, `rot`, `scaleX`, `scaleY`, `alpha`, relative to the rig pose) and part swaps (for example `eyes_open` → `eyes_closed`).
- **Required clips:**

| Clip                   | Used for                         | Motion                         | Loops?              |
| ---------------------- | -------------------------------- | ------------------------------ | ------------------- |
| `sleep`                | Idle                             | Slow breathing, eyes closed    | Yes                 |
| `work`                 | Working                          | Head bob, paws tapping         | Yes                 |
| `wait`                 | Waiting on another agent         | Gentle sway, glancing sideways | Yes                 |
| `step_up`, `step_down` | Moving out of the strip and back | Rise, return                   | Once each           |
| `knock`                | Needs input                      | Paw taps the glass             | Yes, until resolved |
| `alert`                | Failed                           | Still, ears down               | Yes, subtly         |
| `pace`                 | Stalled                          | Small side-to-side steps       | Yes                 |
| `present`              | Ready                            | Holds up the card              | Once, then `rest`   |
| `rest`                 | Ready, after presenting          | Seated, slow blink             | Yes                 |

**Style:**

- Flat vector, at most 6 colors per animal, dark outline.
- Must read on light and dark taskbars.
- No resemblance to known characters or brand mascots, including any AI company's mascot.

**v1 animals:** fox, rabbit, cat and owl, assigned in that order (ADR 0030).

**Shared UI elements:** the "z", the red failure marker, text chips and badges are drawn by the UI, not by each animal.

**Format details** (field names, units, easing, rig-pose visibility, chaining): `art/README.md` (ADR 0017).

**Validation:** `pnpm art:check` MUST verify every animal has every required part and pivot and every clip references valid parts.

### 8.8 Fixtures

- `fixtures/<name>.jsonl` — one line per hook event: `{ "tMs": 0, "hook": "PermissionRequest", "payload": { … } }`.
- `fixtures/<name>.expected.json`:

```json
{
  "timeline": [ { "tMs": 0, "sessionId": "...", "state": "...", "tier": "..." } ],
  "effects": [ { "tMs": 0, "type": "Notify", "sessionId": "...", "reason": "needs_permission" } ]
}
```

**Sanitization MUST happen before commit:**

- Paths become `C:\fixture\<project>`.
- Free-text fields are removed (§8.3).
- Session ids are replaced with stable fake ids.

## 9. Windows platform requirements

### 9.1 Strip window

- Tauri window options: transparent, no decorations, always on top, hidden from the taskbar, not focusable, not resizable, no shadow.
- Then apply Win32 extended styles "no activate" and "tool window", so the strip never takes focus and never appears in Alt-Tab.
- Use Tauri's Windows option that avoids a white flash on transparent windows.
- Never call anything that sets focus to the strip.

**VERIFY V-10:** the strip stays above the taskbar after the taskbar is clicked, and click-through toggling causes no flicker.

### 9.2 Placement

- Use the chosen monitor's work area (the monitor minus the taskbar). The strip's bottom edge sits on the work area's bottom edge, its right edge 8 logical px from the work area's right edge. It grows leftward.
- Strip height by size setting: small 48, medium 64, large 80 logical px. One character slot is as wide as the strip is tall.
- **Taskbar auto-hide:** detect it and rest the strip on the top edge of the shown taskbar, as reported by the shell even while hidden (ADR 0002).
- **Reposition** on display changes, scaling (DPI) changes and work-area changes, with a 5-second polling fallback (FR-15).
- **Maximum width:** 6 slots (5 characters plus the den). Beyond that, the den absorbs the rest.

### 9.3 Click-through

- The strip ignores the mouse by default.
- About 30 times a second, the core reads the cursor position. While the cursor is inside a hit region reported by `set_hit_regions`, the strip accepts the mouse; otherwise it ignores it again.

### 9.4 From a hook request to the session's process and window

1. **Process.** On the first request for a session, and on every `UserPromptSubmit` (ADR 0014), look up which process owns the client end of that loopback connection using the TCP connection table (**VERIFY V-7**). This should be the Claude Code process. Record its PID and start time, so a reused PID is not mistaken for the same process.
2. **Window.** Walk up the parent processes to the first one that owns a visible top-level window. Map every window to its root owner; never pick a `PseudoConsoleWindow`; stop at `explorer.exe`; among several windows prefer one whose title contains the project (ADR 0012). Record the window handle's owning executable and a precision level.
3. **Fallback.** If step 1 fails, use a `SessionStart` command hook that runs `knock` (built so it never opens a console window) to report its own parent chain. Adopting this needs an ADR, because command hooks on Windows run through Git Bash or PowerShell depending on the machine.

### 9.5 Jump precision

| Where Claude Code runs                                                    | What Knock focuses                                                                    | Precision                                                     |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Classic console window (Command Prompt or PowerShell in their own window) | That console window                                                                   | `exact_window`                                                |
| Windows Terminal                                                          | The Windows Terminal window                                                           | `app_window`. The exact tab cannot be identified from outside |
| VS Code, Cursor or another editor's built-in terminal                     | The editor window whose title contains the project name, else the first editor window | `app_window`                                                  |
| JetBrains IDEs and any other host                                         | The owning app's top-level window                                                     | `app_window`                                                  |
| Nothing found                                                             | —                                                                                     | `none`; the click only acknowledges                           |

Selecting the exact Windows Terminal tab is out of scope for v1.

### 9.6 Bringing a window to the front

Windows limits which apps may bring another window to the front.

- From the click on the strip, call `SetForegroundWindow` on the target, then restore it if it was minimized (ADR 0013).
- If none works, flash the target window's taskbar button instead.
- Never simulate keyboard input to work around this.

### 9.7 Other platform services

| Service           | Requirement                                                                                                                                                                                                      |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Process liveness  | Check every known PID each tick, matching the start time                                                                                                                                                         |
| Idle time         | Read the system's last-input time each tick; 10 minutes or more counts as away                                                                                                                                   |
| Notifications     | Windows toast notifications via the Tauri notification plugin; Windows Do Not Disturb applies automatically. Clicking the on-screen toast jumps to the session; a click in the notification center only informs (ADR 0015) |
| Tray              | Notification-area icon showing the count of Needs-you sessions (0-9, then 9+). Menu: Pause notifications, Settings, Quit                                                                                         |
| Single instance   | Only one Knock runs at a time                                                                                                                                                                                    |
| Launch at sign-in | Only if the user agreed in setup or Settings. Turning it on clears a Task Manager "disabled" mark; turning it off removes any `Run\Knock` value that starts Knock's app, from any folder, and leaves others alone (ADR 0025). While it is on, Knock rewrites its entry at start if the entry is missing or stale (ADR 0027) |
| Install           | Per-user NSIS installer; never requires administrator rights. Uninstall removes `Run\Knock` and leaves the Task Manager mark (ADR 0028)                                                                          |
| Firewall          | Must not trigger a firewall prompt (**VERIFY V-11**)                                                                                                                                                             |

## 10. Experience requirements

### 10.1 What each state looks like

| State                      | Animal                         | Text chip                  | Interrupt                          |
| -------------------------- | ------------------------------ | -------------------------- | ---------------------------------- |
| `needs_input` (permission) | `step_up` then `knock`         | "Needs permission: <tool>" | Notification once, escalation once |
| `needs_input` (question)   | `step_up` then `knock`         | "Has a question"           | Same                               |
| `needs_input` (seen, ADR 0016) | Calm pose, no knock        | "Waiting for you"          | None                               |
| `failed`                   | `alert` plus red marker        | "Stopped with an error"    | Notification                       |
| `stalled`                  | `pace`                         | "No progress for <n> min"  | Only if enabled                    |
| `ready`                    | `present` then `rest`          | "Done, ready to review"    | Never                              |
| `working`                  | `work`                         | none (tooltip only)        | Never                              |
| `idle`                     | `sleep`                        | none                       | Never                              |
| `ended_unexpectedly`       | Neutral pose, fades after 60 s | "Session ended"            | Never                              |

- Nothing moves except on a state change, or while in a looping Needs-you clip.
- A calm animal (Working, Ready, Resting) plays one cycle of its clip on a state change, then holds still. Only Needs-you clips keep looping (ADR 0031).
- With Windows "Animation effects" off: no stepping or pacing. Characters switch poses in place and show badges.

### 10.2 Notification text

| Case                 | Title                        | Body                                                    |
| -------------------- | ---------------------------- | ------------------------------------------------------- |
| Permission           | "<project> needs you"        | "Claude Code is asking for permission to use <tool>."   |
| Question             | "<project> needs you"        | "Claude Code has a question for you."                   |
| Escalation           | "<project> is still waiting" | "Claude Code has been waiting for you for <n> minutes." |
| Failed               | "<project> stopped"          | "Claude Code stopped with an error."                    |
| Stalled (if enabled) | "<project> may be stuck"     | "No progress for <n> minutes."                          |

### 10.3 Tooltips, den and plain strip

- **Tooltips** show project, state label, time in state and the decision type when Needs you. Never agent text.
- **Den:** above 5 active sessions, extra sessions go into the den, which shows per-tier counts. Only Needs-you sessions step out of it.
- **Plain-strip mode** uses the same layout with colored pills plus text labels.
- **Away card** lists sessions that entered Ready, Failed or Needs you while the user was away and are still there on return, with their status at return (ADR 0034).

### 10.4 Setup flow

Screens in order:

1. Welcome.
2. Choose a taskbar (only if more than one display is connected).
3. Connect to Claude Code: preview of the settings change, then Confirm.
4. Start Knock when you sign in? (yes/no).
5. Test knock.
6. Done.

Every screen uses plain words, explains why, and offers Back. Setup can be reopened from Settings. **Remove Knock from Claude Code** is always one click away in Settings.

### 10.5 Accessibility

- Every character exposes "<project>, <state label>" to Narrator.
- Text contrast meets WCAG AA on light and dark taskbars.

## 11. Security and privacy rules (all MUST)

1. Server binds to 127.0.0.1 only and requires the token on every request.
2. Hook responses are immediate and never contain decision fields. Knock never approves, denies or answers anything.
3. Free text from Claude Code (messages, prompts, tool inputs and outputs) is dropped at parse time. It is never stored, displayed, logged or put in fixtures.
4. Stored data is limited to: session id, project name, cwd hash, event kind, tool name and category, timestamps, PID and process start time, window owner and precision.
5. Logs go to `%LOCALAPPDATA%\Knock\logs`, rotate daily and keep 7 files. They never contain request bodies, tokens or full paths beyond the project name.
6. The settings file is edited only as described in §8.2.
7. No network access except the updater checking GitHub Releases.
8. No telemetry. Diagnostics are shared only through a **Copy diagnostics** button that the user triggers.
9. Updates are verified with Knock's updater key. The private key lives only in GitHub Actions secrets and never in the repository, in prompts or in an agent session.
10. Dependencies are pinned. `cargo-deny` and `pnpm audit` run in CI.

## 12. Testing

### 12.1 Automated tests (written by the agent that writes the code)

| Layer         | Required coverage                                                                                           | Tool                                           |
| ------------- | ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| Engine        | Every transition, timer, deduplication rule and suppression rule in §8.4                                    | `cargo test`                                   |
| Invariants    | §8.4 invariants over random event sequences                                                                 | proptest                                       |
| Adapter       | Every fixture payload → expected normalized events; free text dropped                                       | insta snapshots                                |
| Replay        | Every fixture → expected timeline and effects; zero missed Needs-you                                        | `knock replay`                                 |
| Hook setup    | Preview, apply, remove, idempotency, invalid JSON, preservation of other entries — on temporary copies only | `cargo test`                                   |
| Platform math | Placement from recorded monitor and work-area data; process-tree walking on recorded trees                  | `cargo test`                                   |
| UI logic      | Store updates, clip choice per state, den grouping                                                          | Vitest                                         |
| UI visuals    | Every state × mode (animals, plain) × taskbar (light, dark) × animation effects (on, off)                   | Playwright screenshots against a mocked bridge |
| Art           | Parts, pivots, clips valid                                                                                  | `pnpm art:check`                               |
| Smoke         | Install, launch, `knock sim`, read the debug snapshot                                                       | Windows CI runner                              |

### 12.2 Manual testing (done by the user)

- Daily use with real Claude Code sessions.
- **Release matrix:**
  - Windows Terminal, a classic console window, VS Code's terminal, and one other editor.
  - 100%, 150% and 200% scaling.
  - Taskbar auto-hide on and off.
  - One and two monitors.
  - Do Not Disturb on and off.
  - Animation effects on and off.
  - Fresh install, update and uninstall.

The lead MUST give the user a short plain-language checklist for each manual test: what to do, and what they should see.

### 12.3 Simulator scenarios (`knock sim <name>`)

| Scenario         | What it plays                                      |
| ---------------- | -------------------------------------------------- |
| `one-all-states` | One session through every v1 state                 |
| `three-knocks`   | Three sessions with overlapping Needs-you episodes |
| `eight-den`      | Eight sessions, exercising the den                 |
| `stall`          | One session that stalls                            |
| `crash`          | One session whose process disappears               |
| `away`           | Events during a 15-minute idle period              |
| `demo`           | The sequence used for the demo video               |

### 12.4 Definition of done (every task)

1. `pnpm check` passes.
2. New behavior has tests per §12.1.
3. No contract in §8 changed without an approved ADR.
4. The lead reviewed the actual diff.
5. `docs/PROGRESS.md` is updated.
6. For anything the user can see or that touches Windows behavior, the user checked it with the lead's checklist.

## 13. Repository, tooling, CI and release

### 13.1 Layout

```text
knock/
  SPEC.md  CLAUDE.md  LICENSE (MIT)  README.md
  crates/knock-core/  crates/knock-cli/
  apps/desktop/src-tauri/  (platform/ inside src)
  apps/desktop/ui/overlay/  ui/settings/  ui/gallery/
  art/animals/  art/clips/
  fixtures/
  docs/adr/  docs/PROGRESS.md
  site/
  .github/workflows/
  .claude/agents/
```

### 13.2 Tooling

- Rust stable, pinned with `rust-toolchain.toml`. Cargo workspace.
- Node LTS and pnpm, pinned in `package.json` (`engines`, `packageManager`). pnpm workspace.
- Formatting and linting: rustfmt, clippy (warnings are errors), Biome, TypeScript strict mode.
- Libraries named in this spec: Tauri 2 (notification, updater, single-instance, autostart plugins), axum, tokio, tracing, the `windows` crate, PixiJS 8, React, Zustand, Vitest, Playwright, proptest, insta. Adding any other runtime dependency requires the lead's approval and a line in `docs/PROGRESS.md`.

### 13.3 Commands

| Command                                      | Does                                                                                                           |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `pnpm dev`                                   | Run the app in development mode                                                                                |
| `pnpm check`                                 | Format check, lint, type check, all Rust tests (unit, property, adapter, replay, setup, platform math), Vitest |
| `pnpm test:visual`                           | Playwright visual tests                                                                                        |
| `pnpm art:check`                             | Validate art                                                                                                   |
| `pnpm gallery`                               | Open the art gallery                                                                                           |
| `pnpm build`                                 | Build the NSIS installer                                                                                       |
| `cargo run -p knock-cli -- sim <scenario>`   | Run a simulator scenario against the running app                                                               |
| `cargo run -p knock-cli -- replay <fixture>` | Replay a fixture and compare with its expected file                                                            |
| `cargo run -p knock-cli -- doctor`           | Run the health check from the command line                                                                     |

### 13.4 CI (GitHub Actions, Windows runners)

| Workflow      | Trigger      | Does                                                                                                                                  |
| ------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------- |
| `ci.yml`      | Pull request | `pnpm check`, `pnpm test:visual`, `pnpm art:check`, `cargo-deny`, `pnpm audit`, Windows build, smoke test                             |
| `nightly.yml` | Push to main | Same, plus an installer artifact                                                                                                      |
| `release.yml` | Tag `v*`     | Build the NSIS installer, sign the update package with the updater key, upload installer plus `latest.json` to a draft GitHub Release |

The installer is unsigned. The user publishes each release by hand.

### 13.5 Versions

| Version     | End of phase | Audience                              |
| ----------- | ------------ | ------------------------------------- |
| v0.1        | 2            | User only                             |
| v0.2        | 3            | User only                             |
| v0.3-beta.N | 4            | 5-10 testers via a GitHub pre-release |
| v0.9        | 5            | Same testers, via auto-update         |
| v1.0        | 6            | Public                                |

Testers MUST be told the installer is unsigned, and asked to check whether Smart App Control is on (it blocks unsigned apps entirely).

## 14. Phase plan

Sizing assumes the user works 15+ hours a week; it is an estimate, not a commitment.

| Phase                             | Weeks | CW delivers                                                                                                                                                                                                                                                                       | UI delivers                                                                                                     | Exit criteria                                                                                                                                      |
| --------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0. Foundations and Windows spikes | 1     | Repo scaffold, `pnpm check`, CI skeleton. Spike A: strip window per §9.1-9.3 with a test rectangle. Spike B: hook → server round trip. Spike C: request → process → window → focus. Logging hook to record sessions, then sanitized fixtures from 10+ real sessions the user runs | Test sprite for spike A                                                                                         | **Gate G0:** all VERIFY items answered in ADRs; spikes A-C work on the user's machine                                                              |
| 1. Core engine                    | 1.5   | `knock-core` (schema, adapter, reducer, timers), `knock replay`, `knock sim`, engine and adapter tests                                                                                                                                                                            | Rig and clip format, first animal draft, gallery skeleton, `art:check`                                          | All replay and property tests pass (FR-1 partial, FR-2, FR-14)                                                                                     |
| 2. Plain overlay                  | 1     | Server, hook setup (§8.2), notifications, tray, placement (§9.2), settings persistence, `sessions-changed`                                                                                                                                                                        | Plain strip, setup flow (§10.4), settings screens                                                               | **Gate G1:** the user keeps it running for one week of real work. **v0.1**                                                                         |
| 3. Animals                        | 2     | Click-through integration (§9.3), performance budgets                                                                                                                                                                                                                             | Pixi renderer, character controller, 4 animals, knock, chips, tooltips, mode toggle, animation-effects handling | Visual tests for every state; user's manual test (FR-3, FR-4, FR-7; ADR 0033). **v0.2**                                                             |
| 4. Interaction and scale          | 1.5   | Jump (§9.4-9.6), escalation, stall, idle detection, away summary, multiple monitors, FR-15                                                                                                                                                                                        | Den, away card, quiet-hours and pause UI                                                                        | `eight-den` scenario reads correctly. **Gate G2:** 5-10 testers, including people who did not build it, complete setup without help. **v0.3 beta** |
| 5. Hardening                      | 1     | `knock doctor` and health check, restart recovery, ended-versus-failed handling, error handling                                                                                                                                                                                   | Health check screen, polish                                                                                     | User's manual matrix passes (FR-12, FR-13). **v0.9**                                                                                               |
| 6. Ship                           | 1     | `release.yml`, updater, `latest.json`                                                                                                                                                                                                                                             | Landing page in `site/`, demo recording via `knock sim demo`                                                    | **Gate G3:** two weeks of dogfooding with zero missed Needs-you events; clean install and update on a fresh Windows 11 machine. **v1.0**           |

**Lead tasks in every phase:**

- Plan and brief the agents.
- Review every change.
- Write ADRs for decisions and VERIFY results.
- Keep `docs/PROGRESS.md` current.
- Produce manual-test checklists for the user.
- In Phase 6, draft the case study on building Knock entirely with Claude Code, from `docs/PROGRESS.md` and the ADRs.

**If a gate fails:**

- **G0, overlay:** apply the Win32 styles directly before considering any other approach.
- **G0, process lookup:** consider the §9.4 fallback, or ship `app_window` precision only (ADR either way).
- **G1:** fix detection or the plain strip before any animal work.
- **G2:** spend Phase 5 on tester issues.

## 15. Phase 0 verification list

Each item is answered in an ADR with evidence (what was run, what happened). Agents MUST NOT guess answers.

| ID   | Question                                                                                                                                                                                                         |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V-1  | Do HTTP hooks reach `127.0.0.1` on native Windows with a literal `X-Knock-Token` header? What is the exact handler schema (timeout units, matcher syntax for tool events, whether a non-blocking option exists)? |
| V-2  | What does Claude Code do when Knock is down, slow, or returns `401`? It must never block or show the user an error                                                                                               |
| V-3  | What are the real payload fields per event on Windows (`session_id`, `cwd`, `hook_event_name`, `source`, `notification_type`, `tool_name`)? Which tool names appear (including PowerShell)?                      |
| V-4  | Which event, if any, signals API errors, rate limits or crashes (for `agent.failed`)?                                                                                                                            |
| V-5  | What, if anything, is sent when the user closes the terminal window?                                                                                                                                             |
| V-6  | Do already-running sessions pick up newly installed hooks, or only sessions started afterwards? (Setup text must say so.)                                                                                        |
| V-7  | Does the connection-owner lookup return the Claude Code process for HTTP hook requests?                                                                                                                          |
| V-8  | Does the parent-process walk find the right window for Windows Terminal, a classic console and VS Code's terminal?                                                                                               |
| V-9  | Which supported technique brings the target window to the front after a click on the non-activating strip?                                                                                                       |
| V-10 | Strip behavior: transparent, never focused, hidden from Alt-Tab and taskbar, stays above the taskbar when the taskbar is clicked, no flicker when click-through toggles at 30 Hz?                                |
| V-11 | Does listening on 127.0.0.1 avoid any firewall prompt?                                                                                                                                                           |
| V-12 | Can clicking a toast notification be routed back to Knock to jump to the session?                                                                                                                                |
| V-13 | How large is the auto-hidden taskbar's reveal zone, and where should the strip sit then?                                                                                                                         |

## 16. Team and working agreement

| Role           | Who                      | Responsibilities                                                                                                                                                                                                                                                                    |
| -------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| User           | Human                    | Makes decisions, approves ADRs and releases, does all manual testing, runs real sessions for fixtures. Does not read code                                                                                                                                                           |
| Lead           | Main Claude Code session | Plans phases, briefs agents, owns §8 contracts, reviews every change against this spec, writes ADRs, keeps `docs/PROGRESS.md`, writes manual-test checklists, asks the user about anything this spec does not settle. Does not write feature code (small integration fixes allowed) |
| `core-windows` | Subagent                 | Everything owned by CW in §7.1. Rust only, plus CI config                                                                                                                                                                                                                           |
| `ui`           | Subagent                 | Everything owned by UI in §7.1. TypeScript, SVG art, landing page                                                                                                                                                                                                                   |

**Lead start-of-session routine:** read `SPEC.md`, `docs/PROGRESS.md` and any ADRs newer than the last progress entry, then state the current phase and the next step.

**Briefs to agents** MUST state:

- the goal
- the spec sections that apply
- the paths in scope
- the acceptance criteria
- the tests required

A task touching both agents' paths MUST be split.

**Agents report back with:**

- what changed and which files
- tests run and their results
- any deviation from the spec
- open questions

Agents MUST NOT change a §8 contract, touch another agent's paths, add runtime dependencies, or edit the user's real Claude Code settings file.

**Review:**

1. The lead reads the actual diff, not just the agent's summary.
2. The lead runs `pnpm check` and the relevant tests.
3. The lead checks the change against this spec and the definition of done (§12.4).
4. **High-risk changes** (Phase 0 spikes, anything in §8.1-8.2, anything in §11) also get a fresh review. The lead runs it with a new agent that has none of the session's context (ADR 0029).

**Branches** (ADR 0029):

- Feature branches start from `develop`. After review, the lead merges them into `develop`.
- The lead never touches `main`.
- The user pushes `develop`, opens a pull request into `main`, reviews the code and merges it.

**Records:**

- `docs/adr/NNNN-title.md` for every decision and VERIFY result.
- `docs/PROGRESS.md` — one entry per working session: date, phase, done, next, open questions for the user.
- These records are also the raw material for the case study.

## 17. References

| Topic                     | Source                                                                                                                 |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Design rationale          | *Knock: Technical Design Document* (the user's design doc)                                                             |
| Claude Code HTTP hooks    | https://pilot-shell.com/blog/hooks-guide · https://www.mintlify.com/jackdog668/claude-code/advanced/hooks              |
| Windows hook shell issues | https://claudeissues.com/issue/32930-bug-hooks-always-executed-via-usr-bin-bash-on-windows-ignoring-shell-setting      |
| Tauri window options      | https://docs.rs/tauri-utils/latest/tauri_utils/config/struct.WindowConfig.html                                         |
| Windows Terminal tabs     | https://learn.microsoft.com/windows/terminal/command-line-arguments · https://github.com/microsoft/terminal/pull/20082 |
| Smart App Control         | https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003               |

Re-check the official Claude Code, Tauri and Microsoft documentation at the start of each phase; all three change often.
