// Dev only: one sample session per v1 state (and Needs-input variant), for the mock
// bridge and the visual tests. Never imported by the shipped strip.
import type { AwayItem, AwaySummary, SessionView } from "../bridge";

const base = {
	since: new Date(Date.now() - 3 * 60_000).toISOString(),
	activity: null,
	needs: null,
	precision: "app_window",
	animal: "fox",
	agent: "claude-code",
	simulated: true,
} as const;

export const SAMPLES = {
	permission: {
		...base,
		id: "permission",
		project: "knock",
		tier: "needs_you",
		state: "needs_input",
		needs: { kind: "permission", label: "PowerShell", seen: false },
	},
	permission_no_tool: {
		...base,
		id: "permission_no_tool",
		project: "api",
		tier: "needs_you",
		state: "needs_input",
		needs: { kind: "permission", label: null, seen: false },
	},
	question: {
		...base,
		id: "question",
		project: "website",
		tier: "needs_you",
		state: "needs_input",
		needs: { kind: "answer", label: null, seen: false },
	},
	seen: {
		...base,
		id: "seen",
		project: "notes",
		tier: "needs_you",
		state: "needs_input",
		needs: { kind: "permission", label: "Bash", seen: true },
	},
	failed: {
		...base,
		id: "failed",
		project: "billing-service",
		tier: "needs_you",
		state: "failed",
	},
	stalled: {
		...base,
		id: "stalled",
		project: "data",
		tier: "needs_you",
		state: "stalled",
	},
	ready: {
		...base,
		id: "ready",
		project: "docs",
		tier: "ready",
		state: "ready",
	},
	working: {
		...base,
		id: "working",
		project: "mobile-app",
		tier: "working",
		state: "working",
		activity: "edit",
	},
	idle: {
		...base,
		id: "idle",
		project: "scripts",
		tier: "resting",
		state: "idle",
	},
	ended: {
		...base,
		// Recent: in animals mode an ended session fades 60 s after `since` (§10.1).
		since: new Date(Date.now() - 20_000).toISOString(),
		id: "ended",
		project: "infra",
		tier: "resting",
		state: "ended_unexpectedly",
	},
} satisfies Record<string, SessionView>;

export type SampleKey = keyof typeof SAMPLES;
export const SAMPLE_KEYS = Object.keys(SAMPLES) as SampleKey[];

/**
 * The end of `knock sim eight-den` (crates/knock-cli/scenarios/eight-den.jsonl, §12.3):
 * eight sessions in the core's order, with the animals the core assigns (ADR 0030).
 * Three need you (a permission, an error, a question), three are ready, two work.
 */
export const EIGHT_DEN: SessionView[] = (
	[
		["s21", "web-shop", "ready", "ready", "fox", null],
		["s22", "api-gateway", "ready", "ready", "rabbit", null],
		["s23", "data-jobs", "needs_you", "needs_input", "cat", "permission"],
		["s24", "mobile-app", "needs_you", "failed", "owl", null],
		["s25", "docs-site", "ready", "ready", "fox", null],
		["s26", "infra", "working", "working", "rabbit", null],
		["s27", "billing", "needs_you", "needs_input", "cat", "answer"],
		["s28", "search", "working", "working", "owl", null],
	] as const
).map(([id, project, tier, state, animal, kind]) => ({
	...base,
	id,
	project,
	tier,
	state,
	animal,
	needs:
		kind === "permission"
			? { kind, label: "PowerShell", seen: false }
			: kind === "answer"
				? { kind, label: null, seen: false }
				: null,
}));

const AWAY_ITEMS: AwayItem[] = [
	{ id: "s21", project: "web-shop", outcome: "ready" },
	{ id: "s23", project: "data-jobs", outcome: "needs_you" },
	{ id: "s24", project: "mobile-app", outcome: "failed" },
	{ id: "s25", project: "docs-site", outcome: "ready" },
	{ id: "s27", project: "billing", outcome: "needs_you" },
	{ id: "s22", project: "api-gateway", outcome: "ready" },
	{ id: "s29", project: "a-project-with-a-very-long-name", outcome: "failed" },
];

/** An `away-summary` with `n` items (up to 7), away for 25 min until now. */
export function awaySample(n = 3, now = Date.now()): AwaySummary {
	return {
		since: new Date(now - 25 * 60_000).toISOString(),
		items: AWAY_ITEMS.slice(0, n),
	};
}
