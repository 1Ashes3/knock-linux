// Dev only: one sample session per v1 state (and Needs-input variant), for the mock
// bridge and the visual tests. Never imported by the shipped strip.
import type { SessionView } from "../bridge";

const base = {
	since: new Date(Date.now() - 3 * 60_000).toISOString(),
	activity: null,
	needs: null,
	precision: "app_window",
	animal: "fox",
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
		id: "ended",
		project: "infra",
		tier: "resting",
		state: "ended_unexpectedly",
	},
} satisfies Record<string, SessionView>;

export type SampleKey = keyof typeof SAMPLES;
export const SAMPLE_KEYS = Object.keys(SAMPLES) as SampleKey[];
