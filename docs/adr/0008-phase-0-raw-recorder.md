# 0008: Phase 0 raw recorder (temporary exception to "never stored")

- **Status:** Approved by user on 2026-10-16
- **Date:** 2026-10-18 (written up after a fresh-review finding)
- **Spec sections:** §3 (Agent messages), §8.8, §11.3, §14 Phase 0
- **VERIFY item:** none

## Context

Phase 0 needs real Claude Code sessions to build sanitized fixtures (§14, §8.8). Raw hook payloads contain agent free text, and §3 says agent messages are never stored. On 2026-10-16 the user allowed raw recordings to exist temporarily on their machine, outside the repo, and that went into `docs/PROGRESS.md`. The fresh review of cdec79b pointed out that an exception to a §3 decision needs an ADR.

## Decision

The Phase 0 recorder is allowed with these limits:

- Debug builds only, and off unless `KNOCK_RECORD_DIR` is set.
- The recording folder must be under `%LOCALAPPDATA%`, which OneDrive doesn't sync, and outside any git repository.
- Raw recordings are never printed, never put in prompts or reports, and never committed. Only sanitized output goes into `fixtures/`.
- Once the fixtures are made, the recorder code and every raw recording are deleted. This is a **Gate G0 item**.

## Consequences

- The recorder must refuse a folder outside `%LOCALAPPDATA%` (a Spike B follow-up).
- `docs/PROGRESS.md` carries "remove recorder and raw recordings" as an open G0 item.
- Incident on 2026-10-17: a Spike B test wrote hook config into this repo's `.claude\settings.local.json` for about 5 minutes. The lead session's own events were recorded during that time. The file was deleted and the token rotated. On 2026-10-18, with the user's approval, every test recording was deleted.

## Evidence

none
