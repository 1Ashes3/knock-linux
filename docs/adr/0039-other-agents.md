# 0039: Other agents (Antigravity, Codex, Copilot)

- **Status:** Proposed
- **Date:** 2026-11-10
- **Spec sections:** §8.1, §8.2, §8.5 (hook routes and setup), §9.4, §11
- **VERIFY item:** V-new (agent hook behaviour, see Evidence)

## Context

Only Claude Code can post to a URL from a hook. The other agents run a command. Sessions of different agents must not collide, and Knock must never block or break an agent.

## Decision

- Add a command-hook helper: `knock-desktop hook <agent> [event]`. It reads the hook's JSON from stdin, posts it to `/v1/hooks/<agent>` with the token, and **always exits 0**, even if Knock isn't running or its output pipe is closed. For Antigravity it prints `{}`; for the others nothing.
- Add one adapter per agent in `knock-core` with allow-listed fields (ADR 0009). Sessions are named `<agent>-<id>`; notifications name the agent.
- Setup per agent uses the same preview, backup, confirm, atomic write flow as Claude (§8.2). Knock recognises its own hook entries by exact program name (`knock`, `knock-desktop` or a `Knock*.AppImage`, then `hook <agent>`).
- **Antigravity:** one `"knock"` suite in `~/.gemini/config/hooks.json`, events SessionStart, PreInvocation, PostToolUse, PostInvocation, Stop. **Knock never registers PreToolUse**: that hook is a gate, and `{}` is turned into "deny", which locked the agent out of every tool (2026-11-09). Stop never answers block or continue (that re-prompts the agent). A new conversation in the same Antigravity window replaces the old session (it has no end event).
- **Status in the UI:** Claude Code supported; Antigravity beta, partly working (it cannot show permission knocks); Codex and Copilot written but untested against the real programs and greyed out.

## Consequences

- SPEC.md §8.1 and §8.2 need the new route, the helper and the agent setup (not edited yet; needs the user's approval, §8 changes always do).
- Copilot's HTTP hooks need `COPILOT_HOOK_ALLOW_LOCALHOST=1`, so Knock uses the helper there too.
- Hooks of agents other than Claude are only written after the user confirms the preview. The real config files were touched once, for Antigravity, with the user's approval.

## Evidence

Antigravity v1.5.3: events and payloads captured in an isolated lab and read from the binary; the lockout, its cause and the fix confirmed live by the user; an isolated run of the five remaining hooks against the release binary passed (`reviews/release-check.md` in the shared folder). Codex and Copilot: payload shapes from their documentation only, not run.
