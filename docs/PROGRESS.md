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

## 2026-10-18: Phase 0 (Spike B)
Done:
- Spike B (branch `phase-0/spike-b`, cdec79b): `core-windows` built the §8.1 server inside the desktop app.
  - The server binds 127.0.0.1 only, keeps its port and token in `server.json`, and serves the token-checked hook and health endpoints.
  - Hooks get `200 {}` at once and are processed on a worker afterwards.
  - Logs rotate daily (a small hand-written `tracing` subscriber).
  - A debug-only recorder is included for Phase 0 (ADR 0008).
  - Lead review and `pnpm check` passed, and the fresh review is done.
- ADRs, all approved by the user on 2026-10-18 except 0008, which the user approved on 2026-10-16:
  - 0005: no SessionStart HTTP hook (V-1).
  - 0006: handler schema and a 1 s timeout (V-1, V-2, V-6, V-11).
  - 0007: StopFailure is the source of `agent.failed` (V-4).
  - 0008: rules for the Phase 0 raw recorder.
  - SPEC.md §6, §8.2, §8.3 and §8.4 were updated to match.
- Incident (ADR 0008): for about 5 minutes, a test wrote hook config into this repo's `.claude\settings.local.json`. The file was deleted, the token rotated, and every test recording deleted with the user's approval on 2026-10-18. The real `%USERPROFILE%\.claude\settings.json` was never touched.
- More ADRs, approved by the user on 2026-10-18: 0009 (V-3, V-5), 0010 (state-model fixes from the recordings), 0011 (16 MB body limit, never an error reply). SPEC.md §8.1, §8.2, §8.3 and §8.4 were updated to match.
- User manual tests on 2026-10-17 and 2026-10-18 passed: a real interactive session, Knock stopped mid-session, the window closed with X, no firewall prompt, and two permission prompts in default mode.
- Follow-up brief sent to `core-windows` for fresh-review findings 3-5 and the recorder rules: token check before routing, `server.json` validation and a durable write, event timestamps taken in queue order, recorder only under `%LOCALAPPDATA%`.
- Second fresh review (21ec9eb, 704dcdc):
  - Finding 1, an over-limit upload was reset: fixed. The server now reads and discards the rest of the body before replying, and a test covers it.
  - Finding 2: ADR 0011's wording was narrowed to the hook endpoint.
- Spike B is complete.
Next:
- Spike C: request → process → window → focus (V-7, V-8, V-9).
Dependencies added: serde_json 1.0.154 (user-approved), axum 0.8.9, tokio 1.53.3, tracing 0.1.44 (§13.2). New transitive crates: axum-core, matchit, httpdate, tokio-macros.
Clarifications (lead):
- An over-limit body currently returns `413`; this is pending the user's decision.
- `fill_random` is a free function in `platform/`, not a trait method. That's accepted for Stage A.
- Logging uses a hand-written subscriber, because `tracing-subscriber` and `tracing-appender` aren't on the §13.2 list.
- Gate G0 items carried forward:
  - Remove the Phase 0 recorder and all raw recordings (ADR 0008).
- Must be settled before Phase 2:
  - Handle the case where the fixed port can't bind: Hyper-V, WSL2 and Docker reserve blocks of ports in 49152-65535, and those blocks can change at reboot. This needs the Repair flow and a visible error (fresh-review finding 7).
- `sessionCount` and `lastEventAt` are spike-only and get redone in Phase 2.
Open questions for the user:
- none (body limit decided: ADR 0011)

## 2026-10-17: Phase 0 (Spike A)
Done:
- Spike A (branch `phase-0/spike-a`, d81fc40):
  - `core-windows` built the Tauri shell (`knock-desktop`): the §9.1 strip window, placement on the primary work area, and Win32 click-through driven by a 30 Hz cursor poll.
  - `ui` built the test page with one target.
  - Lead review passed, the fresh-session review is done, and the user's manual checks 1-5 passed.
- Fresh-review finding 1 (tao can rewrite the strip's styles): fixed by the lead with a small integration fix. The styles are now re-asserted every poll tick (ADR 0001).
- `cargo deny check` was run locally for the first time (cargo-deny 0.20.2): advisories, bans, licenses and sources are all OK. It only warned about duplicate crate versions and unused license allow-list entries.
- ADRs, all approved by the user on 2026-10-17: 0001 V-10 strip behavior, 0002 V-13 auto-hide placement, 0003 hit-region shape for §8.5, 0004 allow MPL-2.0. SPEC.md §8.5 and §9.2 were updated to match ADRs 0003 and 0002.
Next:
- Spike B: hook → server round trip (V-1, V-2, V-3, V-6, V-11).
Dependencies added: tauri 2.12.4, tauri-build 2.7.3, windows 0.62.2 (named in §13.2). serde 1.0.229 (approved by the user 2026-10-16; serde_json is approved too but not used yet). Dev tools: Vite 8.3.4, @tauri-apps/cli 2.12.3.
Clarifications (lead):
- Deferred from the Spike A review, to Phase 2 (§9.2) and Phase 4 (FR-15), not lost:
  - repositioning on display, DPI and work-area changes, with the 5 s polling fallback
  - auto-hide placement, per ADR 0002
  - the chosen monitor instead of always the primary one
- Before Phase 2:
  - set a real CSP in `tauri.conf.json`; it is `null` in the spike (§11)
  - replace `eprintln!` with tracing logs (§11.5)
  - final identifier and icon
- Memory was 154 MB in a dev build, against the 150 MB budget in §6. Measure a release build in Phase 3.
Open questions for the user:
- none

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
