# 0033: The latency and CPU budgets are goals, not tests

- **Status:** Approved by user on 2026-11-04
- **Date:** 2026-11-04
- **Spec sections:** §6 (Latency, CPU), §14 (Phase 3 exit, Phase 5 "performance")
- **VERIFY item:** none

## Context

ADR 0032 deferred the Phase 3 latency and CPU measurement. On 2026-11-04 the user passed every v0.2 manual test and said: "CPU / Memory limit testing is no longer needed."

## Decision

No latency or CPU measurement is run in any phase. As with memory (ADR 0026), the §6 Latency and CPU numbers stay as design goals. This replaces ADR 0032.

## Consequences

- SPEC.md §6 Latency and CPU each end with "A goal, not a test (ADR 0033)".
- §14 Phase 3 exit becomes "Visual tests for every state; user's manual test". "performance" is removed from the Phase 5 CW work.
- Briefs no longer ask for latency or CPU measurements. The render loop still stops when nothing moves (ADR 0031).

## Evidence

none
