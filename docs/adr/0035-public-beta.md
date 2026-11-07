# 0035: v0.3 is a public beta, and the user judges Gate G2

- **Status:** Approved by user on 2026-11-06
- **Date:** 2026-11-06
- **Spec sections:** §13.5 (Versions), §14 (Gate G2)
- **VERIFY item:** none

## Context

§13.5 plans v0.3-beta.N for "5-10 testers via a GitHub pre-release". On 2026-11-06 the user decided: "Testers will be public. I want to release this as a beta release and allow public to test it." Gate G2 counted 5-10 testers, which doesn't fit an open beta.

## Decision

- v0.3-beta.N is a public GitHub pre-release. Anyone can download it.
- Testers report through GitHub issue forms: a "Setup report" and a "Bug report". Neither asks for agent content (§11).
- Gate G2 passes when the user judges that enough people who didn't build Knock have completed setup without help, based on the setup reports and open bugs. There's no fixed number (user decision).
- Before the first public release, the lead runs a fresh security review (ADR 0029 process) of §8.1, §8.2, §9.7 and §11 (user decision).

## Consequences

- SPEC.md §13.5: the v0.3-beta.N audience becomes "Public, via a GitHub pre-release (ADR 0035)".
- SPEC.md §14 Gate G2 becomes "people who did not build it complete setup without help, judged by the user from setup reports (ADR 0035)".
- The repository must be public for the release to be downloadable. The user does that, along with the merge to `main`, the tag and the release.
- The README becomes the public entry point. It carries the unsigned-installer and Smart App Control notes that §13.5 requires.
- There is no auto-update until Phase 6, so beta testers download each new beta by hand.

## Evidence

none
