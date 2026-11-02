# 0025: Which sign-in entry Knock adds and removes

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §9.7 (Launch at sign-in)
- **VERIFY item:** none

## Context

§9.7 says Knock starts at sign-in "only if the user agreed". The entry is the per-user value `HKCU\...\Run\Knock`. Windows also keeps a separate "disabled" mark for it under `Explorer\StartupApproved\Run`, which Task Manager sets. Three cases weren't settled:

- the user disabled Knock in Task Manager, then turned the setting on in Knock;
- the `Knock` value was written by an older copy of Knock in another folder;
- the value was written by some other program under the same name.

The fresh review of 4b9fda9 also found that the rule matched a program named `Knock.exe`, while the built app is `knock-desktop.exe`.

## Decision

- **On:** always write Knock's own value (the quoted path of the running exe), and clear the Task Manager "disabled" mark (user decision, Phase 2).
- **Off:** remove the value if it starts Knock's app, from any folder: the program's file name is the same as the running app's file name, ignoring letter case. The "disabled" mark is cleared too. A value that starts any other program is left alone (user decision, 2026-11-02).
- Both plain and expandable string values count.

## Consequences

- SPEC.md §9.7 "Launch at sign-in" becomes: "Only if the user agreed in setup or Settings. Turning it on clears a Task Manager 'disabled' mark; turning it off removes any `Run\Knock` value that starts Knock's app, from any folder, and leaves others alone (ADR 0025)."
- The match uses the app's real file name, taken from the running exe, not a fixed name.

## Evidence

none (decision). The rules are unit-tested in `platform/mod.rs`, and the registry test (`KNOCK_AUTOSTART_TEST=1`) covers them on the real key.
