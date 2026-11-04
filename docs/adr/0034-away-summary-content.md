# 0034: The away card shows each session's status on return

- **Status:** Approved by user on 2026-11-04
- **Date:** 2026-11-04
- **Spec sections:** §5 (FR-9), §8.5 (`away-summary`, `dismiss_away_summary`), §9.7 (Idle time), §10.3
- **VERIFY item:** none

## Context

FR-9 says the card "lists sessions that became Ready, Failed or Needs you during the idle period". The spec doesn't say what happens to a session that reached one of those states and then left it before the user returned. It also doesn't say which outcome to show if a session passed through several of them.

## Decision

The user chose "show current status" on 2026-11-04:

- A session is listed if, during the idle period, it entered `ready`, `failed`, `needs_input` or `stalled`, and when the user returns it is still in one of those states.
- `outcome` comes from its state at return: `ready` → "ready", `failed` → "failed", `needs_input` or `stalled` → "needs_you".
- A session that was already in one of those states before the user left, and never left it, is not listed.
- `since` is when the idle period started. No event is sent when no session qualifies.
- The card folds away on a click or after 60 s. Either way the UI calls `dismiss_away_summary()`. A new `away-summary` replaces any card still showing.

## Consequences

- SPEC.md §10.3 gets an "Away card" bullet that points here.
- `core-windows` implements detection and the summary, and `ui` implements the card. They meet at the existing §8.5 event and command. No contract changes.

## Evidence

none
