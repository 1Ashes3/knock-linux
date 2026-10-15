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
