# 0005: No SessionStart hook; the first event starts a session

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §8.2, §8.3, §8.4
- **VERIFY item:** V-1 (part), V-3 (`source`)

## Context

§8.2 registers `SessionStart` as an HTTP hook. Claude Code 2.1.301 doesn't support HTTP hooks for that event. Getting `SessionStart` any other way would need a command hook, the §9.4 fallback, which runs through Git Bash or PowerShell depending on the machine.

## Decision

- Drop `SessionStart` from the §8.2 event list.
- A session starts on its first event (§8.4 "any first event"), usually `UserPromptSubmit`.
- `session.started` and its `source` stay in §8.3 as kinds with no producer in v1.
- Revisit if Spike C adopts the §9.4 command-hook fallback.

## Consequences

- A newly opened Claude Code window appears in the strip when the user sends its first message, not when the window opens. FR-1 still holds: the session appears within 1 s of its first hook event.
- Knock can't tell startup, resume, clear and compact apart.
- SPEC.md edits:
  - §8.2: remove `SessionStart` from the event list.
  - §8.3: the `SessionStart` mapping row is marked "not available over HTTP (ADR 0005)".

## Evidence

Windows 11 Pro 25H2 (build 26200.7462), Claude Code 2.1.301, Spike B server (cdec79b). The hooks were in a scratch project's `.claude\settings.local.json`.

On every run, both `claude -p` and `--resume`, the debug log said `Skipping HTTP hook <url> — HTTP hooks are not supported for SessionStart`. The server received nothing for that event.

All the other registered events arrived over HTTP. The non-HTTP route was not tested.
