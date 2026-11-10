// Which agent a session belongs to (ADR 0039). Claude Code is the default and is never
// named in the UI; another agent's name shows on the hover card and in the accessible name.
import type { SessionView } from "./bridge";

const NAMES: Record<string, string> = {
	"claude-code": "Claude Code",
	codex: "Codex",
	copilot: "GitHub Copilot",
	agy: "Antigravity",
};

/** The agent's display name, or the id for one this build doesn't know. */
export function agentName(agent: string): string {
	return NAMES[agent] ?? agent;
}

/** The name to show for `s`, or null for Claude Code (so its cards stay as they were). */
export function shownAgent(s: SessionView): string | null {
	return s.agent && s.agent !== "claude-code" ? agentName(s.agent) : null;
}
