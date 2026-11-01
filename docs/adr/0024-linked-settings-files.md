# 0024: Linked Claude Code settings files

- **Status:** Proposed
- **Date:** 2026-11-02
- **Spec sections:** §8.2
- **VERIFY item:** none

## Context

The fresh review of 421599a found that the atomic write replaced a symbolic link with a plain file. For a user whose `settings.json` links into a dotfiles folder, the link would be cut and the dotfiles copy would lose Knock's entries. A file with several hard links would be split the same way. acf7763 fixed both, but §8.2 doesn't describe the new behavior, so the fresh review of acf7763 asked for an ADR.

## Decision

- If the settings file is a symbolic link (or sits in a linked folder), Knock reads, previews, backs up and writes the file the link points to. The preview shows that target's path.
- If the file has more than one hard link, setup stops with a plain message and writes nothing, as for invalid JSON (§8.2 step 1).
- Debug builds refuse when the resolved target is the user's real Claude Code settings file, so a link can't get around the development rule.

## Consequences

- SPEC.md §8.2 step 1 gains: "A file with more than one hard link is refused the same way. A symbolic link is followed, and its target is edited and shown in the preview (ADR 0024)."
- The debug-build guard is part of the next `core-windows` brief.

## Evidence

Tests in `hooks.rs` (acf7763) with a real symbolic link and a real hard link on Windows 11 with Developer Mode on.
