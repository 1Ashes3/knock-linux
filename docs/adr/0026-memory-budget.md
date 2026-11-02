# 0026: The memory budget is a goal, not a test

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §6 (Memory)
- **VERIFY item:** none

## Context

§6 says "Under 150 MB total, including WebView2" but doesn't say how to count it. The Phase 2.4 measurements passed or failed depending on the measure. Almost all of the memory is WebView2; Knock's own process uses about 6 MB.

## Decision

No memory test is run. The user said on 2026-11-02: "Hopefully this won't use much memory. We don't need to test this." If memory is ever checked, the number that counts is Task Manager's "Memory" column (private working set) for Knock and its WebView2 processes.

## Consequences

- SPEC.md §6 Memory becomes: "Under 150 MB total, including WebView2, as Task Manager's Memory column shows it. A goal, not a test (ADR 0026)."
- Briefs no longer ask for memory measurements.

## Evidence

The Phase 2.4 release build (8d1f1aa) on Windows 11 Pro 10.0.26200 with WebView2 156.0.4314.58 used 94-109 MB private working set, with the plain strip and the setup window. Private bytes were 172-199 MB, and the sum of working sets was 366-413 MB.
