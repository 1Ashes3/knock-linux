# 0003: Shape of `set_hit_regions` regions

- **Status:** Approved by user on 2026-10-17
- **Date:** 2026-10-17
- **Spec sections:** §8.5, §9.3
- **VERIFY item:** none

## Context

§8.5 names the `set_hit_regions(regions)` command but doesn't define what a region looks like. In Spike A the core and the UI agreed on a shape in their briefs. The fresh review asked for that shape to be written into the contract so later UI work can't drift from it.

## Decision

`regions` is an array of `{ x, y, width, height }`:

- All four values are numbers in logical pixels, and fractions are allowed.
- `x` and `y` are measured from the strip's top-left corner.
- Each call replaces the previous set of regions. An empty array makes the whole strip click-through.
- A point counts as inside a region if it is on the left or top edge. Points on the right or bottom edge are outside.

## Consequences

- SPEC.md §8.5: the `set_hit_regions(regions)` row gets "regions: `[{ x, y, width, height }]`, logical px, relative to the strip; replaces the previous set".
- No code change. Spike A already uses this shape.

## Evidence

none
