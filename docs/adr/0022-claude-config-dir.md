# 0022: Follow CLAUDE_CONFIG_DIR when it is set

- **Status:** Proposed
- **Date:** 2026-11-02
- **Spec sections:** §8.2
- **VERIFY item:** none

## Context

§8.2 fixes the file as `%USERPROFILE%\.claude\settings.json`. Claude Code honours the environment variable `CLAUDE_CONFIG_DIR`, which moves its settings folder. For a user who sets it, Knock would edit a file Claude Code never reads. "Connected to Claude Code" would pass, Knock would never hear from a session, and the user would see no reason why. The fresh review of 421599a found this.

## Decision

If `CLAUDE_CONFIG_DIR` is set to a non-empty value in Knock's environment, Knock uses `<CLAUDE_CONFIG_DIR>\settings.json`. Otherwise it uses `%USERPROFILE%\.claude\settings.json`. The setup preview always shows the path it will edit.

## Consequences

- SPEC.md §8.2: "File: `%USERPROFILE%\.claude\settings.json`" becomes "File: `<CLAUDE_CONFIG_DIR>\settings.json` if that variable is set, else `%USERPROFILE%\.claude\settings.json` (ADR 0022)".
- Debug builds keep using only `KNOCK_CLAUDE_SETTINGS`.
- If the user changes the variable later, the health check's "last heard from a session" will show the problem, and running setup again fixes it.

## Evidence

none. Claude Code's documentation lists `CLAUDE_CONFIG_DIR`. This is not verified by a run here.
