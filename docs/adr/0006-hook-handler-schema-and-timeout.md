# 0006: HTTP hook handler schema and a 1-second timeout

- **Status:** Approved by user on 2026-10-18
- **Date:** 2026-10-18
- **Spec sections:** §6 (Agent impact), §8.1, §8.2
- **VERIFY item:** V-1, V-2, V-6, V-11

## Context

V-1 asks for the exact handler schema. V-2 asks what Claude Code does when Knock is down, slow or returns `401`.

Spike B found that HTTP hooks always block until they get a reply or hit the timeout. There is no non-blocking option. So a hung Knock delays every hook by the full timeout. With §8.2's `timeout: 2`, that is 2 s on every tool call.

## Decision

- The §8.2 handler shape stays as written, with `timeout: 1`. The timeout is in seconds, and fractions are accepted.
- Tool events use the matcher `"*"`.
- The token stays in a literal `X-Knock-Token` header. The token-in-path fallback isn't needed.

## Consequences

- SPEC.md §8.2:
  - `"timeout": 1`.
  - Matcher for tool events: `"*"`.
  - "Exact field names and units are VERIFY V-1" becomes "timeout in seconds (ADR 0006)".
- SPEC.md §6 Agent impact: add "If Knock is running but hung, each hook waits at most the 1 s timeout."
- Setup copy must say: "Claude Code windows that are already open connect within about 10 seconds; no restart needed." This settles V-6.
- Still open:
  - What an interactive session shows on screen when a hook fails. The manual test checks it.
  - What happens when a payload is over the body limit (fresh-review finding).

## Evidence

**Setup:** Windows 11 Pro 25H2 (build 26200.7462), Claude Code 2.1.301, Spike B debug server. Hooks in a scratch project's `.claude\settings.local.json`. Runs used `claude -p --model haiku`, timed from `--debug-file`, stream-json hook events and the server log.

**V-1, the handler schema:**
- With a literal `X-Knock-Token`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Notification`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop` and `SessionEnd` all arrived and got `200`.
- Timeout units: with a 5 s server delay, `timeout: 2` cancelled at 2000-2004 ms and `timeout: 0.5` at 500-503 ms.
- Matchers: `"*"`, `""` and leaving the matcher out all matched every tool.
- Non-blocking: `"async": true` was accepted and ignored. Each hook still blocked for the full timeout.

**Reply time:**
- Server side: p50 18 µs and p95 33 µs over 75 hooks. That covers handler time only.
- Client side: Claude Code reported 0-10 ms per hook round trip. Both are far under §6's 100 ms.

**V-2:** the same small session ran under each condition. It always finished with exit 0, and the tool ran.

| Condition | Cost per hook | What Claude Code recorded |
| --- | --- | --- |
| Knock not running | 2-6 ms | Connection refused |
| Knock returns `401` | 0-7 ms | Recorded as an error |
| Knock delays 5 s, timeout 2 | The full timeout | Cancelled |

- In the slow case, 5 hooks added about 10 s to the session.
- In `-p` mode, only a failed `SessionEnd` printed anything: one stderr line.

**V-6:**
- One session kept open across three turns:
  - Turn 1, no hooks: 0 events.
  - Hooks added, then 10 s wait. Turn 2: 4 events with the same session id.
  - Hooks removed. Turn 3: 0 events.
- An already-running interactive session picked up a newly written project settings file within about 9 s. This was the Spike B incident, logged in `docs/PROGRESS.md`.

**V-11:**
- The listener was only on `127.0.0.1:60447`.
- The firewall was on, with notify-on-listen enabled for all profiles.
- No firewall rule mentions Knock, no Windows Security Alert window existed, and no firewall log event relates to Knock.
- All of this was checked without admin rights. The user still has to confirm on screen.
