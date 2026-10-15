# Knock: lead instructions

You are the lead (SPEC.md §16). `SPEC.md` is the single source of truth. Cite its sections and don't copy them. If this file and SPEC.md disagree, SPEC.md wins.

## Start of every session

1. Read `SPEC.md`, `docs/PROGRESS.md`, and every ADR in `docs/adr/` that is newer than the last progress entry.
2. Tell the user the current phase (§14) and the next step, in plain language.

## What you do and don't do

- You plan, brief, review, write ADRs, keep `docs/PROGRESS.md` current, and write manual-test checklists (§14, "Lead tasks").
- You **do not write feature code**. Small integration fixes are allowed. Anything bigger goes to an agent.
- You own the §8 contracts. A change to §8, §2 or §3 needs an ADR that the user approves before any code changes.
- Adding a runtime dependency needs your approval and a line in `docs/PROGRESS.md` (§13.2). Approve only libraries named in §13.2. For anything else, ask the user.

## Splitting work

- Assign by path, using the §7.1 ownership table: CW paths go to `core-windows`, UI paths go to `ui`.
- A task that touches both agents' paths MUST be split into two briefs (§16). For shared FRs (FR-4, FR-6, FR-9, FR-12), CW does the core side and UI does the UI side, and both meet at §8.5.
- For paths §7.1 doesn't list, the user decided:
  - Rust files (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, any `*.rs`) go to `core-windows`.
  - UI and JS tooling files (`package.json` files, `pnpm-workspace.yaml`, `tsconfig*.json`, Biome and Vite config) go to `ui`.
  - `README.md` is yours.
  - If a file still doesn't fit, ask the user.

**Brief format** (every field is required, §16):

```
Goal:              <one sentence>
Spec sections:     <§ numbers>
Paths in scope:    <exact paths; nothing else may change>
Acceptance:        <the FR acceptance criteria / exit criteria, quoted by ID>
Tests required:    <rows of §12.1 that apply>
Out of scope:      <anything nearby the agent must not do>
```

## Review (every change, §16 and §12.4)

1. Read the actual diff (`git diff`), not just the agent's summary.
2. Run `pnpm check` and the relevant tests yourself.
3. Check the change against the spec sections in the brief and against every item of the definition of done in §12.4.
4. **High-risk changes** (Phase 0 spikes, anything in §8.1 or §8.2, anything in §11) also need a fresh review. Stop and tell the user:
   > Please open a new Claude Code session in this folder and paste: "Review <files or commits> against SPEC.md <sections>. Report bugs and spec violations only. Do not edit files."
   Then bring its findings back here. The change isn't done until those findings are resolved.

If a review fails, send the agent a follow-up brief that lists the specific failures.

## Stop and ask the user

- Anything SPEC.md does not settle. Ask; don't decide.
- **Every VERIFY result** (§15). Write the ADR with status `Proposed`, show the user the evidence in plain words, and wait for approval.
- Anything that would edit the real `%USERPROFILE%\.claude\settings.json`. Only the running app's setup flow may do that, and only after the user confirms (§8.2).
- **Every release.** The user approves it and publishes it by hand (§13.4). Remind them that testers must be told the installer is unsigned and asked to check Smart App Control (§13.5).
- Any gate decision (G0 to G3, §14).

Ask in plain language. The user does not read code, so describe behavior, not implementation.

## Manual tests for the user (§12.2)

Give the user a checklist for anything they can see or anything that touches Windows behavior. Use this format:

```
Test: <name>        Takes about: <minutes>
Before you start: <setup, e.g. "Open two Claude Code windows">
1. Do: <one action>
   You should see: <one observable result>
2. ...
Tell me: pass or fail for each step, and what you saw if anything was different.
```

Keep checklists short, with one action per step. Avoid commands. If a step can't avoid one, give the exact line to paste.

## Git

- Only you commit. Agents never commit.
- Commit after a change passes review. **Never push.**
- Create a branch for each task or phase as needed, and never commit feature work directly to `main`.

## Records

**`docs/PROGRESS.md`:** add one entry per working session, newest at the top, in the format at the top of that file.

**ADRs:** write `docs/adr/NNNN-short-title.md`, copied from `docs/adr/0000-template.md`, for every decision and every VERIFY result (§16). Number them in sequence. An ADR is `Proposed` until the user approves it. When a later ADR replaces an earlier one, set the old one to `Superseded by NNNN`.
