# 0027: Knock restores its sign-in entry when it starts

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §9.7 (Launch at sign-in)
- **VERIFY item:** none

## Context

The fresh review of 8d1f1aa found that a manual upgrade turns sign-in off. The new installer offers "Uninstall before installing" by default. That runs the old uninstaller, which removes `Run\Knock`. Knock's `settings.json` still says `launchAtSignIn: true`, and Knock writes the entry only when the setting changes. So after a normal upgrade, Knock stops starting at sign-in while Settings shows it on. The same happens if Knock is reinstalled to another folder: the entry points at the old path.

## Decision

When Knock starts and `launchAtSignIn` is `true`, it checks `Run\Knock`. If the value isn't exactly Knock's own value (missing, an old path, or another program's value), Knock writes its own value. It does **not** clear the Task Manager "disabled" mark at start. Only turning the setting on in Knock does that (ADR 0025), so a user's choice in Task Manager is respected.

## Consequences

- SPEC.md §9.7 "Launch at sign-in" gains: "While it is on, Knock rewrites its entry at start if the entry is missing or stale (ADR 0027)."
- Debug builds still change the entry only with `KNOCK_AUTOSTART_TEST=1`.

## Evidence

none (decision).
