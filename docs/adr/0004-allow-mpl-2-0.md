# 0004: Allow MPL-2.0 dependencies

- **Status:** Approved by user on 2026-10-17
- **Date:** 2026-10-17
- **Spec sections:** §3 (License), §11.10
- **VERIFY item:** none

## Context

On Windows, Tauri 2 pulls in five crates under MPL-2.0: `cssparser`, `cssparser-macros`, `dtoa-short`, `selectors` and `option-ext`. `deny.toml` didn't allow that license, so `cargo deny check` would fail in CI.

## Decision

Add `MPL-2.0` to `licenses.allow` in `deny.toml`.

## Consequences

- MPL-2.0 is copyleft only for the individual files under it. Knock stays MIT and we don't modify those files.
- If we ever change an MPL-2.0 file, we must publish that file's changes.

## Evidence

none (licenses were found with `cargo tree`; see the Spike A report in `docs/PROGRESS.md`).
