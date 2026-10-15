# Progress log

One entry per working session, newest first (SPEC.md §16). Use this format:

```
## YYYY-MM-DD: Phase N
Done:
- <what was finished, with ADR numbers>
Next:
- <the next step>
Dependencies added: <name, reason, who approved; or "none">
Clarifications: <spec clarifications made by the lead; or "none">
Open questions for the user:
- <question, or "none">
```

---

## 2026-10-16: Phase 0 (session 2)
Done:
- Phase 0 task 1, repo scaffold. `core-windows` set up the Cargo workspace (`knock-core`, `knock-cli`), pinned Rust to 1.99.0, and added `deny.toml` and the CI skeleton (`ci.yml`). `ui` set up the pnpm workspace (`@knock/ui`), TypeScript (strict), Biome, Vitest and the root `pnpm check`. The lead reviewed the diff and `pnpm check` passed.
- Lead added `LICENSE` (MIT), with the user's approval.
Next:
- Spike A: strip window with a test rectangle (§9.1-9.3, V-10). CW does the Tauri shell; UI does the test sprite.
- Spike B: hook → server round trip (V-1, V-2, V-3, V-6, V-11). Spike C: process → window → focus (V-7, V-8, V-9).
Dependencies added: dev tools only (TypeScript 7.0.2, Biome 2.5.15, Vitest 5.0.4). No runtime dependencies.
Clarifications (lead):
- CI installs cargo-deny with `cargo install --locked`. The cargo-deny action is a container action and can't run on Windows runners.
- "Dependencies are pinned" (§11.10) is met by the committed `Cargo.lock` and `pnpm-lock.yaml`, plus exact versions in `package.json`. Cargo.toml version requirements don't need `=`.
- pnpm's minimum-release-age check stays on. Don't add `minimumReleaseAgeExclude`; pin an older version instead.
- `cargo deny check` hasn't run locally yet. It first runs in CI.
Open questions for the user:
- none

## 2026-10-16: Phase 0 (not started)
Done:
- Wrote CLAUDE.md, the `core-windows` and `ui` agent definitions (both Opus), this log and the ADR template.
Next:
- Phase 0 task 1 (branch `phase-0/scaffold` created): brief `core-windows` on the Rust workspace + CI skeleton, and `ui` on the pnpm/TS/Vite tooling and root scripts, in parallel. Then Spikes A-C.
Clarifications (lead):
- Vite is the dev-only bundler for the UI pages (dev tool, not a runtime dependency).
Dependencies added: none
Clarifications (user decisions):
- Files not listed in §7.1: Rust files go to `core-windows`, UI and JS tooling files go to `ui`, README.md goes to the lead.
- Phase 0 raw recordings may exist unsanitized on the user's machine temporarily, outside the repo. They are sanitized before anything goes into `fixtures/`.
- Git: the lead commits and never pushes. Branches are created as needed.
Open questions for the user:
- none
