# 0029: Merge into develop; fresh reviews run by the lead

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §16
- **VERIFY item:** none

## Context

Up to v0.1, the lead merged finished phases into `main`. High-risk changes got a fresh review that the user started by hand in a new Claude Code session, and the user pasted the findings back. From Phase 3 on, the user wants work to continue while they're away, and wants to review code before it reaches `main`.

## Decision

- Feature branches start from `develop`. After the lead's review (and the fresh review, when one is needed), the lead merges them into `develop`.
- The lead never touches `main`. The user pushes `develop`, opens a pull request into `main`, reviews it and merges it.
- The lead runs fresh reviews itself, with a new agent that starts with none of the session's context. A fork or a resumed agent doesn't count. The lead verifies each finding before acting on it.
- Everything else stays the same: only the lead commits, the lead never pushes, and the "stop and ask the user" list in `CLAUDE.md` still applies.

## Consequences

- SPEC.md §16: Review step 4 and a new "Branches" paragraph.
- `CLAUDE.md`: the Review and Git sections.
- `develop` was created from `main` at v0.1.0 (181f9ea).

## Evidence

none (decision by the user on 2026-11-02).
