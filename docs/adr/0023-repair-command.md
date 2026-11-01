# 0023: The Repair flow and its command

- **Status:** Approved by user on 2026-11-02
- **Date:** 2026-11-02
- **Spec sections:** §8.1, §8.5
- **VERIFY item:** none

## Context

§8.1 says: if the fixed port can't be bound later, show a plain-language error with a **Repair** button that picks a new port and re-runs hook setup, with a preview. §8.5 has no command for this. Phase 0 noted that Hyper-V, WSL2 and Docker reserve blocks of ports in 49152-65535, and those blocks can change at reboot. So this will happen on real machines.

## Decision

- If the server can't start, the core keeps running. It shows the tray tooltip "Knock can't listen for Claude Code. Open Settings to repair." and opens the settings window with `?repair=1`.
- New command `repair_server()` → `{ ok: true } | { ok: false, message }`. It picks a new free port in the §8.1 range, keeps the token, saves `server.json` and starts the server.
- After a successful repair, the UI runs the usual Connect step (preview, then Confirm), because Claude Code's hooks still point at the old port.
- `setup_status()` gains `serverError: string | null`, holding plain-language text.

## Consequences

- SPEC.md §8.5: add the row `repair_server()`, "Pick a new port and restart the server (§8.1, ADR 0023)". Add `serverError` to `setup_status`.
- `core-windows` implements the core side and `ui` implements the repair screen, both in Phase 2.
- Health check: "Knock is listening for Claude Code" fails, and its text points to Repair.

## Evidence

none
