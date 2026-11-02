# 0028: Uninstall removes the sign-in entry and keeps the Task Manager mark

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §9.7 (Launch at sign-in, Install)
- **VERIFY item:** none

## Context

8d1f1aa and ab4550f added an uninstall hook that ran `knock-desktop.exe --remove-sign-in` and saved and restored the `Run\Knock` value, so another program's value would survive (ADR 0025). The fresh review of ab4550f found two problems:

- A manual upgrade runs the old uninstaller. The hook cleared the Task Manager "disabled" mark, and ADR 0027 then restored the entry without it. So an upgrade undid the user's Task Manager choice.
- If the app couldn't run during uninstall, the hook wrote back Knock's own value, leaving an entry that points at a deleted exe.

## Decision

The uninstaller uses Tauri's default only: it deletes `Run\Knock` and never touches the Task Manager mark (`StartupApproved\Run\Knock`). The custom hook logic, the `--remove-sign-in` flag and its test are removed. The only hook left unticks "Create desktop shortcut".

ADR 0025's "leaves others alone" applies to turning the setting off in Knock, not to uninstalling.

## Consequences

- An upgrade keeps the user's Task Manager choice. ADR 0027 restores the entry at start, and the mark still applies.
- A real uninstall leaves the mark behind if the user had set it. Windows ignores a mark that has no entry.
- In the very unlikely case that another program uses the value name `Knock`, uninstalling Knock removes that value.
- SPEC.md §9.7 "Install" gains: "Uninstall removes `Run\Knock` and leaves the Task Manager mark (ADR 0028)."

## Evidence

none (decision). The review of ab4550f is the trigger.
