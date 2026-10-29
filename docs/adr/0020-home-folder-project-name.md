# 0020: A home-folder session's project is "home"

- **Status:** Proposed
- **Date:** 2026-10-29
- **Spec sections:** §8.3, §11.4, §11.5
- **VERIFY item:** none

## Context

§8.3 says `project` is the last segment of `cwd`. For a session started in the user's home folder (`C:\Users\<name>`), that segment is the Windows user name. The name would then show in the strip, the log and the diagnostics, and before the sanitizer fix it could also reach a fixture. In Phase 0 the lead applied a "home" rule to the fixture sanitizer and the desktop app, and the Phase 1 adapter copied it. All of this happened without an ADR. The fresh review of the core flagged that.

## Decision

`project` is the last segment of `cwd`, after removing `.` and applying `..`. It becomes `home` when:

- the segment's parent is `Users` (any case), or
- the segment equals `%USERNAME%` or the last segment of `%USERPROFILE%` (case-insensitive).

The adapter takes the user names as an input, so `knock-core` stays pure.

## Consequences

- SPEC.md §8.3: "`project` is the last segment of `cwd`" becomes "`project` is the last segment of `cwd`, or `home` for the user's home folder (ADR 0020)."
- No code change. The adapter, `jump.rs` and the fixture sanitizer already do this.
- A project folder that happens to share the user's name also shows as `home`. This is accepted.

## Evidence

none
