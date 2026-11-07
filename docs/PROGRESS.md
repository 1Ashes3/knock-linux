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

## 2026-11-06: Phase 4, public beta preparation
Done:
- ADR 0035 (approved by the user): v0.3 is a **public** beta via a GitHub pre-release, and the user judges G2 from setup reports. SPEC.md §13.5 and §14 were updated.
- README written for the public, with screenshots in docs/images (lead).
- `core-windows`: GitHub issue forms, "Setup report" and "Bug report" (313eca8). Neither asks for agent content (§11).
- Fresh pre-release review of §8.1, §8.2, §9.7 and §11 (ADR 0029 process, user request). No serious findings. Six minor ones were verified by the lead. The user chose:
  - Fixed now (72939db):
    - (1) An undone sign-in turn-off no longer clears the Task Manager mark. The mark is now cleared only after the save.
    - (3) A second launch opens Settings.
    - (6) CI runs `cargo fetch --locked`.
  - (2) A manual upgrade with "Run Knock" unticked leaves sign-in off until Knock is started once. This is now a README note.
  - **Deferred to Phase 5:**
    - (4) Setup reopens at every start after the hooks are removed while sign-in is on.
    - (5) Misleading messages: the server error text for an invalid server.json; "development build" text when USERPROFILE is unset; the health check says "Choose Repair" while no Repair button is shown.
- History scan before going public: no secrets or personal paths, only test placeholder tokens. `pnpm audit` is clean. cargo-deny runs in CI.
- 2026-11-07, ADR 0036 (approved by the user): a manual `release.yml` (90e73e6).
  - Run from `main`, it builds the installer on GitHub and creates a draft pre-release with the SHA-256 and the notes from `docs/releases/`.
  - It never publishes.
  - The build job is read-only; only the release job can write.
  - Reviewed by the lead and the user; action SHAs verified against their tags. Not yet run on GitHub.
- 2026-11-08: the user ran the Release workflow and **published v0.3.0-beta.1** as a public GitHub pre-release. The sidebar shows a tag count, not "Latest", because it is a pre-release; this is expected.
- 2026-11-08: the user tested the GitHub-built installer and passed the quick check:
  - Upgrade keeps settings.
  - A second launch opens Settings.
  - Setup works.
- README badges, added by the user: the beta badge works. The shields.io license badge showed "repo not found", a stale cache from when the repo was private (GitHub's API reports MIT). The user chose to leave it as is.
Next:
- Gate G2: the user judges it from setup reports.
- Gate G2: the user judges it from setup reports.
Dependencies added: none
Clarifications: none
Open questions for the user:
- G2 verdict, once setup reports come in.
- Later: should the app show its version (Settings or diagnostics)? Testers currently read it from the installer name.

## 2026-11-04 (evening): Phase 4 (interaction and scale)
Done:
- Survey: FR-5 escalation, FR-6 jump (with the flash fallback and the toast click), FR-11 stall, suppression, multiple monitors and FR-15 were already built and tested (checked by `core-windows`). The quiet-hours and pause controls were in Settings and the tray since Phase 2.
- ADR 0034, approved by the user: the away card lists sessions that entered Ready, Failed or Needs you while the user was away and are still there on return. A seen session that asks again while the user is away is listed too (user, same day).
- 4.1 (`core-windows`): idle time in the platform layer, and a pure `Away` tracker in knock-core. `away-summary` is sent on return, and `dismiss_away_summary()` clears it (8472163, e1eba3e).
  - A debug-only `KNOCK_AWAY_SECONDS` shortens the 10 minutes for manual tests.
- 4.2 (`ui`): the den in both modes (1ff0a25). It shows per-tier counts, and only Needs-you sessions step out.
  - Slot rule: Needs-you first, then the core's order. The lead accepted this because it matches FR-10.
- 4.2 (`ui`): the away card, above the strip.
  - Lead review caught the card placed beside the strip, past the core's 6-slot width cap. It was moved, and the dev mock now applies the cap.
- Lead review: `pnpm check` passes, 216/216 screenshot tests, `art:check` passes, 17/17 replays.
- Merged into `develop`: phase-4/away-core, phase-4/away-reask, phase-4/den-away-ui.
- Version set to 0.3.0-beta.1.
Next:
- The user's manual test of the den, eight-den and the away card.
- Then G2: build the beta, the lead writes tester instructions (unsigned installer, Smart App Control, §13.5), and the user publishes the GitHub pre-release by hand.
Dependencies added: none
Clarifications (lead):
- A PC that sleeps before 10 idle minutes produces no away summary. Accepted for v1.
- The den is not clickable.
- The away card shows at most 5 items, then "and N more".
- 2026-11-06: the user passed the Phase 4 manual test, all 9 steps:
  - The den and eight-den read correctly (Phase 4 exit criterion).
  - Plain-mode den.
  - The away card appears on return, closes on a click, and folds after 60 s.
Open questions for the user:
- The G2 tester list.

## 2026-11-04 (later): Phase 3 done, Gate G1 passed
Done:
- **Gate G1 passed** (user decision, 2026-11-04).
- Version bumped to 0.2.0 (2f01cf2). The user built the installer on their v0.1 machine and passed every v0.2 checklist step (1-16):
  - Upgrade over v0.1, uninstall and reinstall.
  - Hover box goes away, and clicks pass through.
  - Second-monitor placement is also tested now.
  - No issues found.
- **Phase 3 exit criteria are met.**
- ADR 0033 (approved by the user): latency and CPU are goals, not tests. It supersedes 0032. SPEC.md §6 and §14 were updated.
Next:
- Plan Phase 4 (interaction and scale, v0.3 beta, Gate G2).
Dependencies added: none
Clarifications: none
Open questions for the user:
- none

## 2026-11-04: Phase 3 (animals)
Done:
- The user asked to start Phase 3 while the Gate G1 week is still running. G1 still ends with the user's pass/fail. If it fails, §14 says to fix detection or the plain strip before more animal work.
- ADRs approved by the user:
  - 0030: the v1 animals are fox, rabbit, cat and owl, in that order.
  - 0031: calm animals play one cycle of their clip, then hold still. Only Needs-you clips loop.
  - SPEC.md §8.7 and §10.1 were updated to match.
- 3.2 (`ui`): rabbit, cat and owl art (899ee72). The user approved the art from the review images.
  - The lead fixed two artCheck tests that assumed the fox was the only animal.
- 3.3 (`core-windows`): the engine assigns fox, rabbit, cat, owl from one `knock_core::ANIMALS` list (bd86d06).
  - 4 new engine tests. 17/17 replays still match.
- 3.1 (`ui`): animals mode with a PixiJS renderer (83855ed):
  - The controller picks clips from `state` and `needs.seen` only.
  - Chips show the §10.1 texts, with leader lines.
  - Tooltips, the red marker, and badges when Animation effects are off.
  - Narrator names, hit regions, and a live animals/plain switch (FR-3, FR-4 UI side, FR-7).
  - Lead review: `pnpm check` passes, `pnpm test:visual` 188/188, `art:check` passes.
  - Two plain-strip baselines were refreshed. The only differences were anti-aliasing specks after an Edge update (lead, integration fix).
- Merged into `develop`: phase-3/animals, phase-3/core-animals, phase-3/renderer. `main` was not touched.
- 3.4 latency and CPU measurement: started, then stopped at the user's request. The unfinished work was discarded and `phase-3/perf` deleted (ADR 0032). It never launched a test build.
Next:
- Build a v0.2 test installer from `develop` and give the user one checklist: animals in every state, hover and tooltip going away (needs real Windows), mode switch, sizes, Animation effects off, upgrade from v0.1, uninstall (ADRs 0027 and 0028).
- At the end of the G1 week: ask whether G1 passes.
Dependencies added: pixi.js 8.22.0 (UI runtime, named in §13.2, approved by the lead).
Clarifications (lead):
- An animal id with no art falls back to the fox.
- A seen `needs_input` and `ended_unexpectedly` hold the rig pose.
- The unused `waiting_agent` and `communicating` states use `wait`, settled.
Open questions for the user:
- The v0.2 manual test (checklist to follow).
- Later, before Phase 5 or G3: run the deferred performance measurement? (ADR 0032)

## 2026-11-02: Phase 2 (plain overlay), v0.1
Done:
- 2.1: engine wired into the app, plain strip, tray and app icons (d7a661a). Review fixes (be83e04): tray pause follows the in-effect value; the strip stays hidden until it is placed.
- 2.2: hook setup (§8.2), setup commands, settings window, sign-in entry (421599a, 35a5824). Review fixes (acf7763): HKCU-only sign-in, link handling, the file's own indentation is kept, a race before the rename.
- ADRs approved: 0022 (CLAUDE_CONFIG_DIR), 0023 (Repair), 0024 (linked settings files). SPEC.md §8.2 and §8.5 were updated to match.
- 2.3: Repair, stale-hook check, placement on the chosen display with auto-hide and FR-15 re-placement, and sign-in ownership (948fa53, 9556fc2). Settings got a "Which taskbar" choice (user request, e061464). Fresh-review fixes are in 4b9fda9.
- User manual tests passed: setup preview, Repair screen preview, strip placement at 100/150/200% scaling, auto-hide on and off, and the Settings taskbar line.
- 2.4: per-user NSIS installer, 2.65 MB, unsigned, version 0.1.0 (8d1f1aa). Fresh-review fixes are in ab4550f and 3c2d25b. ADRs approved: 0025 (sign-in entry), 0026 (memory budget is a goal, not a test), 0027 (sign-in entry restored at start) and 0028 (uninstall uses Tauri's default).
- The user installed the release build and ran the real setup. All steps passed:
  - no administrator prompt; the desktop shortcut box was unticked
  - the real Claude Code settings were changed only after Confirm
  - toasts showed "Knock" with the fox icon
  - a real permission request showed "Allow?", and clicking it jumped to the session
  - the health check was all green
  - the Task Manager "disabled" mark was cleared by turning the setting on in Knock
- The user approved **v0.1** on 2026-11-02 (local only, user only, §13.5). Merged into `main` and tagged `v0.1.0` locally. Not pushed.
- Process change (ADR 0029, user decision):
  - From now on, reviewed branches merge into `develop`, and the lead never touches `main`.
  - The user pushes `develop` and merges into `main` through a reviewed pull request.
  - The lead runs fresh reviews itself, with a new agent that has no session context.
  - `develop` was created from v0.1.0. SPEC.md §16 and `CLAUDE.md` were updated to match.
Next:
- **Gate G1** started on 2026-11-02: the user runs v0.1 for at least one week of real work. The user asked for no Phase 3 planning until after it.
- Second-monitor checks (placement steps 8-9) wait until the user has a second display.
- Not tested yet: uninstall and a manual upgrade (ADRs 0027 and 0028). Test them at the v0.2 upgrade.
Dependencies added:
- UI: react 19.3.1, react-dom 19.3.1, zustand 5.0.16 (§13.2). Dev only: @playwright/test 1.64.1 and @types/react.
- Rust: tauri-plugin-single-instance 2.5.5 (§13.2), the tauri `tray-icon` and `image-png` features (these bring in the `image` crate), and extra `windows` crate features.
- tauri-plugin-autostart was added, then removed: its HKLM risk led to a direct HKCU Run value.
- All approved by the lead.
Clarifications (lead):
- The debug snapshot uses views.
- Free functions in `platform/` are fine alongside the trait.
- `sessions.json` leaves out simulated sessions.
- The §10.2 notify texts use the singular "1 minute" (user approved).
- The server can't stop on its own once started (axum 0.8), so there is no runtime "stopped" text.
Open questions for the user:
- At the end of the G1 week: does G1 pass?

## 2026-10-29: Phase 1 (core engine)
Done:
- `core-windows` built the `knock-core` engine and `knock replay` (b66c13f):
  - the §8.3 event, and the `claude-code` adapter with the allow-list
  - the §8.4 engine, kept pure
  - proptest invariants and insta adapter snapshots
  - hand-written `.expected.json` files for all 17 fixtures. All replay exactly, and the engine agreed with the hand derivation on the first run.
- `ui` built the §8.7 format, a first fox draft, the gallery (`pnpm gallery`), the shared animated sleep "z" (the user asked for it) and `pnpm art:check` (dd1dd19). The user reviewed the fox and the z: LGTM.
- `core-windows` applied ADR 0018 and built `knock sim` with all 7 §12.3 scenarios, live or `--local` (aea12c3). 115 Rust tests pass and every scenario has zero missed Needs-you.
- ADRs approved by the user on 2026-10-29: 0017 (art format details) and 0018 (gaps in the §8.4 transitions). SPEC.md §8.4 and §8.7 were updated to match.
- **The Phase 1 exit criteria are met:** all replay and property tests pass (FR-1 partial, FR-2, FR-14).
- Fresh review of the core is done. The user approved ADRs 0019 (several pending requests; only the main agent clears Failed), 0020 (home project name) and 0021 (two ADR 0019 edge cases) on 2026-11-01, and SPEC.md §8.3 and §8.4 were updated to match.
  - `core-windows` fixed everything in 7b25c98 and 7ee86e2:
    - requester sets, and per-requester dedup
    - `session_id` and `agent_id` charset-checked
    - wrong-typed optional fields count as missing
    - an explicit `Input::SetSuppressed`
    - the replay guard sees a new episode that replaces a seen one
  - Result: 124+ Rust tests, 17/17 replays (`subagent` updated per ADR 0021), and 7/7 scenarios with no missed Needs-you.
- Merged into `main` (fast-forward, local only).
Next:
- Plan Phase 2 (plain overlay, v0.1, Gate G1).
- The fresh review's finding 7 is deferred: the adapter's install, uninstall and health-check implementations land in Phase 2 with §8.2 and FR-12.
- The app must send `SetSuppressed` before the first event and on every settings change. Agent ids from `Needs::waiting()` must not be persisted (§11.4).
- A background answer after the user acknowledged Ready brings the session back to Ready, with a batched ReadyBatchAdd, never a notification (lead reading of ADR 0021).
Dependencies added: proptest 1.12.0 and insta 1.49.0 (dev-only, §13.2). knock-core uses serde and serde_json (approved).
Clarifications (lead):
- The app works out whether notifications are suppressed (pause or quiet hours in local time) and passes it with each `Tick`, so the engine stays pure.
- First-event implied states:
  - activity gives working
  - turn.completed gives ready, with ReadyBatchAdd
  - needs.* gives needs_input, with Notify
  - agent.failed gives failed, with Notify
  - anything else gives idle
- An episode is one stay in a Needs-you state, so needs_input followed by failed sends a new `Notify(failed)`.
- `Persist` is emitted once per input whenever the session set, the state or the episode changed.
- Dedup is one-way: only the `permission_prompt` Notification can be the duplicate.
- A question's PreToolUse and PermissionRequest (about 20 ms apart) both count as requests for ADR 0018 item 1. This is accepted, because a click between them is unlikely.
- A `permission_prompt` Notification never opens a new episode while a seen episode is open.
- "Known process" means some event carried `context.pid`. That's the app's job in Phase 2.
- Phase 2 items:
  - a live way to end a sim session for the `crash` scenario (needs an ADR)
  - `knock sim --speed` speeds only the sim's own pacing, not the app's timers; the demo may lower stallMinutes
  - leave room above the strip for the sleep "z", which floats above the 64×64 box
  - the Claude Code hooks docs now list a `Notification` type `agent_needs_input` (a background session waiting on input). Test whether it's a Needs-you source.
  - set HTTP hook `onFailure: "continue"` explicitly
  - the gallery's clip player ignores reduced motion (the z respects it)
Open questions for the user:
- none

---

## 2026-10-28: Gate G0 passed, Phase 1 starts
Done:
- **Gate G0 approved by the user on 2026-10-28.**
  - All 13 VERIFY items are answered in approved ADRs 0001-0016.
  - Spikes A-C and the V-12 toast click work on the user's machine.
  - There are 17 sanitized fixtures.
  - The recorder and every raw recording are deleted.
- With the user's OK, the lead deleted the throwaway test folder `%LOCALAPPDATA%\Knock\phase0-project`. Its project-local hook config held the token.
- All Phase 0 branches were merged into `main` (fast-forward, local only, not pushed).
Next:
- Phase 1, core engine (§14):
  - CW: `knock-core` (schema, adapter, reducer, timers), `knock replay`, `knock sim`, the engine and adapter tests, and the `.expected.json` files.
  - UI: the rig and clip format, the first animal draft, the gallery skeleton, `art:check`.
  - Exit criteria: all replay and property tests pass (FR-1 partial, FR-2, FR-14).
Carried forward:
- Phase 2:
  - the Repair flow, and port reservations on the dynamic range
  - repositioning (FR-15) and the chosen monitor
  - ADR 0002 auto-hide placement
  - a real CSP, the final identifier and icon
  - the installer AUMID so toasts show "Knock" (ADR 0015)
  - the engine's `Notify` replaces the toast spike
- Phase 3: a release-build memory check (dev build was 154 MB).
- G2: check jumping on testers' machines (ADR 0013).
Dependencies added: none
Clarifications (lead): none
Open questions for the user:
- none

---

## 2026-10-25: Phase 0 (fixtures)
Done:
- The user recorded 16 sessions in the test folder, following the lead's checklist (option A), plus a deny/Esc retest.
- `core-windows` built `knock fixtures sanitize` and a guard test that scans `fixtures/`, then wrote 15 fixtures (6fb9522). The lead sanitized 2 more (4234a4c): `deny-then-wait` and `esc-then-wait`. 17 fixtures in total, and the lead review passed.
- The fresh review is done. Its findings (a repeated-key bypass, guard coverage, the home-folder cwd, the sanitizer and checker disagreeing, a line number, the exit code) went to `core-windows`, together with removing the Phase 0 recorder (ADR 0008).
- ADR 0016, approved by the user on 2026-10-25: clicking a Needs-you character marks it seen, with no knock and no escalation. Reason: deny and Esc send no hook. SPEC.md §8.4, §8.5 and §10.1 were updated to match.
Next:
- Review the follow-up.
- Done 2026-10-25, with the user's OK: the lead deleted all 5 raw recording files (`phase0-rec`, `fixtures-rec`) and confirmed no `*.jsonl` remains under `%LOCALAPPDATA%\Knock`, `%APPDATA%\Knock` or the scratchpad. The recorder code was removed in ddd6b89. The ADR 0008 G0 item is closed.
- Fresh review of ddd6b89 is done. Its findings were fixed in 3625935:
  - `.expected.json` files must match the exact §8.8 shape.
  - A cwd named like the user's name gives `home`, in fixtures and in the app.
  - An empty fixture folder fails `fixtures check`.
- Phase 0 work is complete. Gate G0 is waiting on the user's decision.
- Gate G0 decision.
Dependencies added: none new. knock-cli now uses serde_json and serde, both already approved.
Clarifications (lead):
- A fixture `cwd` directly under `\Users\` gets the project `home`, so no username can get into a fixture.
- `.expected.json` files come in Phase 1, together with the engine.
- Findings for Phase 1, from the fixtures:
  - `--continue` reuses the session id after `SessionEnd`.
  - A background subagent can raise a PermissionRequest while the session is Ready, and Knock should knock for it.
  - An automatic `UserPromptSubmit` can follow a subagent's result.
  - Window close and `/exit` both send `SessionEnd`, with reason `other` and `prompt_input_exit`.
Open questions for the user:
- none

---

## 2026-10-18: Phase 0 (V-12 toast spike)
Done:
- Branch `phase-0/v12-toast`:
  - ADR 0014 is implemented: the owner is looked up on every prompt and re-walked only when it changed.
  - A debug-only toast is shown for each PermissionRequest, and clicking it jumps to the session (a94132d).
- The fresh review is done. The lead fixed its 3 findings as small integration fixes:
  - A failed first lookup is logged again.
  - Toasts use the question text for AskUserQuestion.
  - Test header strings now end in CRLF.
- User manual test passed:
  - A click on the on-screen toast brought the console to the front (SetForegroundWindow).
  - Clicks in Win+N do nothing.
  - Do Not Disturb holds back the pop-up, and the notification stays listed.
  - The dev build shows as "Windows PowerShell".
- ADR 0015 (V-12) approved by the user on 2026-10-18. SPEC.md §9.7 updated.
- **All 13 VERIFY items are now answered in approved ADRs:** 0001, 0002, 0005-0007, 0009, 0010, 0012, 0013, 0015.
Next:
- Fixtures: record 10+ of the user's real sessions, then sanitize them per §8.8 and ADR 0008.
- Remove the Phase 0 recorder and the raw recordings.
- Gate G0 decision (user).
Dependencies added: tauri-plugin-notification 2.7.0 (named in §13.2; it brings in notify-rust 4.18.2 and tauri-winrt-notification 0.8.1).
Clarifications (lead):
- §10.2 doesn't cover a permission request with no tool name. The body is then "Claude Code is asking for permission." The question tool uses §10.2's question text (ADR 0010).
- Phase 2: check that the NSIS installer's Start menu shortcut carries the app identity, so toasts show "Knock" (ADR 0015).
Open questions for the user:
- none

---

## 2026-10-18: Phase 0 (Spike C)
Done:
- Spike C (branch `phase-0/spike-c`, dd87727 to 5fc4624):
  - `core-windows` built the connection-owner lookup and the parent walk to the window, plus `get_sessions` and `jump_to_session`.
  - `ui` updated the test page: a session label, and click to jump.
- The fresh review is done. Its findings were fixed: ADR 0013 cleanup, sim requests skip the lookup, the log is trimmed to the owner, and ADR 0012's evidence wording was corrected.
- The lead made small integration fixes to the V-9 diagnostics (712fdbb, a67b3a5). They showed the first "SetForegroundWindow failed" readings were a check made too early.
- ADRs 0012 (V-7, V-8) and 0013 (V-9) were approved by the user on 2026-10-18. SPEC.md §9.4 and §9.6 were updated to match.
- User manual tests passed in four runs: classic console (normal and minimized), Windows Terminal (normal and minimized) and VS Code's terminal. The final run used the cleaned-up code.
Next:
- ADR 0014: re-check the process on every prompt (user approved on 2026-10-18). It goes to `core-windows` together with V-12.
- Remaining Phase 0 work:
  - V-12, clicking a toast to jump.
  - Fixtures from 10+ real sessions, sanitized.
  - Remove the recorder and the raw recordings (ADR 0008).
- Then the Gate G0 decision.
Dependencies added: none. Only `windows` crate features changed.
Clarifications (lead):
- Before UI work builds on it, the TypeScript `SessionView` type must match §8.5 exactly (fresh-review finding 5).
- The `simulated` flag lands with the engine snapshot (Phase 1).
- The stored owner exe stays (§11.4); SessionView will use it later.
Open questions for the user:
- none

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
