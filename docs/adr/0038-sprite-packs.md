# 0038: Sprite packs

- **Status:** Proposed
- **Date:** 2026-11-10
- **Spec sections:** §3 (settings), §8 (commands and the pack protocol), §8.7 (art), §11 (security)
- **VERIFY item:** none

## Context

The four built-in animals were the only characters. Users want other sets of characters, and they should be able to install one without unpacking folders.

## Decision

A pack is a full cast of characters that the user picks in **Settings → Characters**.

- A pack is one `.knockpack` file (a zip). Installing it: **Add a pack…**, drag and drop, or double-click (the MIME type `application/x-knockpack`).
- Characters are either vector rigs (the existing SVG parts format) or sprite sheets (PNG frame strips with a `sheet.json`). A pack may carry its own clips and an optional themed `den.svg`.
- Every pack is checked before it can be used, and served only through a `knockpack` protocol that refuses paths outside the pack. SVGs with scripts, `foreignObject`, external references or `on*` attributes are rejected. Zip entries are checked for traversal, symlinks and size (zip-bomb guard). Invalid packs are listed with the reason and can't be selected.
- Four built-in packs ship (Chibi Girls, Pocket Garden, Whispering Fairies, Pixel Fairy Girls) next to the original "classic" set.
- Settings gains `pack` (default `classic`) and `idleMotion` (default on; a few wing flaps every 3 to 8 seconds while a session works, sleep is unchanged). Both have serde defaults so old settings files still load.
- Switching packs re-skins running sessions with no restart.
- New runtime dependencies: `zip` and `flate2` (only for reading `.knockpack`). Neither is named in §13.2, so each needs the user's approval.

## Consequences

- SPEC.md needs the pack format, the two new settings and the new commands (not edited yet; needs the user's approval).
- The idle flourish reuses each character's `work` clip; a per-pack `idle` clip is possible later.
- Pack art authors follow `art/README.md`; a separate authoring guide is not written yet.

## Evidence

A security review of the pack checks found six issues, all fixed (`Pack checks: fix all six findings`). Packs verified on the strip on KDE Wayland: rig and sprite-sheet characters, live switching, the themed den, one-file install.
