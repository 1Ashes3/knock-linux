# 0032: The Phase 3 performance measurement is deferred

- **Status:** Superseded by 0033
- **Date:** 2026-11-04
- **Spec sections:** §6 (Latency, CPU), §14 (Phase 3 exit criteria)
- **VERIFY item:** none

## Context

The §14 exit criteria for Phase 3 include "latency and CPU budgets met". On 2026-11-04 `core-windows` started building a measurement tool and a side-by-side test build. The user stopped it: "perf should not be carried out. let's skip that for now."

## Decision

Phase 3 can exit without measuring the §6 latency and CPU budgets. The budgets themselves stay in §6, and the animals mode is still built to meet them: the render loop stops once everything has settled (ADR 0031), and it runs at 30 fps at most. The unfinished measurement work was discarded, and `phase-3/perf` was deleted.

## Consequences

- The Phase 3 exit is: visual tests for every state, plus the user's manual test of v0.2.
- The lead asks the user again before any later phase or release that depends on the measured numbers. At the latest that is Phase 5 ("performance") or Gate G3.
- SPEC.md is not changed. This is a deferral, not a change to the budgets.

## Evidence

none
