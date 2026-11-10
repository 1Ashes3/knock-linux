// "Introduce sprites" (ADR 0040): a short demo of the selected pack's characters, drawn in
// place of the real sprites. Pure: the demo is a value, and what it shows at a time is a
// function of that value and the time, so every step can be tested.
//
// The demo never touches real session state. Its sprites are made up here, are local to
// the overlay, and never reach the core: no jump target, no dismiss, no notification.
import type { SessionView, State, Tier } from "./bridge";

/** The introduction: every character, asleep. */
export const INTRO_MS = 3000;
/** How long each picked character spends in each state. */
export const STATE_MS = 3000;
/** How many characters go through the states. */
export const PICKED = 4;

/** The states the picked characters go through, in order (ADR 0040). */
export const DEMO_STATES = [
	"idle",
	"working",
	"needs_input",
	"failed",
	"ready",
] as const satisfies readonly State[];

export type DemoState = (typeof DEMO_STATES)[number];

/** §8.4: each demo state's tier. */
const TIER: Record<DemoState, Tier> = {
	idle: "resting",
	working: "working",
	needs_input: "needs_you",
	failed: "needs_you",
	ready: "ready",
};

/** The tool named on the demo's permission chip. Made up here; never agent text. */
export const DEMO_TOOL = "Edit";

/** The demo's id prefix, so a demo sprite can never be taken for a real session. */
export const DEMO_ID = "intro-demo:";

export interface DemoCharacter {
	id: string;
	displayName: string;
}

export interface Demo {
	startedAt: number;
	/** Every character of the pack, in its order: the introduction. */
	all: DemoCharacter[];
	/** The ones that go through the states. */
	picked: DemoCharacter[];
	/** Real sessions already needing the user when the demo started (see `interrupts`). */
	urgentAtStart: ReadonlySet<string>;
}

export type DemoPhase = "intro" | "states" | "over";

export interface DemoFrame {
	phase: DemoPhase;
	/** Which state the picked characters are in; null outside "states". */
	state: DemoState | null;
	/** The sprites to draw instead of the real sessions; empty once over. */
	sessions: SessionView[];
	/** When the frame changes next; Infinity once over. */
	nextAt: number;
}

/** `n` characters of `chars` in random order, or all of them if there are fewer. */
export function pick<T>(
	chars: readonly T[],
	n: number,
	random: () => number = Math.random,
): T[] {
	const out = [...chars];
	const k = Math.min(n, out.length);
	for (let i = 0; i < k; i++) {
		const j = i + Math.floor(random() * (out.length - i));
		[out[i], out[j]] = [out[j] as T, out[i] as T];
	}
	return out.slice(0, k);
}

/** A real session that needs the user and hasn't been looked at yet. */
function urgent(s: SessionView): boolean {
	return s.tier === "needs_you" && s.needs?.seen !== true;
}

/** One need: the same session in the same state since the same time. */
const needKey = (s: SessionView) => `${s.id}|${s.state}|${s.since}`;

/** The keys of the real sessions that need the user now. */
export function urgentKeys(real: readonly SessionView[]): Set<string> {
	return new Set(real.filter(urgent).map(needKey));
}

/** Starts a demo at `now` with the pack's characters; null if the pack has none. */
export function startDemo(
	chars: readonly DemoCharacter[],
	now: number,
	real: readonly SessionView[],
	random: () => number = Math.random,
): Demo | null {
	if (!chars.length) return null;
	return {
		startedAt: now,
		all: [...chars],
		picked: pick(chars, PICKED, random),
		urgentAtStart: urgentKeys(real),
	};
}

/** When the demo is over. */
export function demoEnd(demo: Demo): number {
	return demo.startedAt + INTRO_MS + DEMO_STATES.length * STATE_MS;
}

/**
 * The safety stop: a real session started needing the user after the demo began (a new
 * knock or a new failure). One that already needed the user when the tray item was
 * clicked doesn't stop it; it shows again when the demo ends a few seconds later.
 */
export function interrupts(demo: Demo, real: readonly SessionView[]): boolean {
	for (const s of real)
		if (urgent(s) && !demo.urgentAtStart.has(needKey(s))) return true;
	return false;
}

function sprite(
	c: DemoCharacter,
	state: DemoState,
	since: number,
): SessionView {
	return {
		id: `${DEMO_ID}${c.id}`,
		agent: "claude-code",
		project: c.displayName,
		tier: TIER[state],
		state,
		since: new Date(since).toISOString(),
		activity: null,
		needs:
			state === "needs_input"
				? { kind: "permission", label: DEMO_TOOL, seen: false }
				: null,
		precision: "none",
		animal: c.id,
		simulated: true,
	};
}

/** What the demo shows at `now`. */
export function demoAt(demo: Demo, now: number): DemoFrame {
	const t = now - demo.startedAt;
	if (t < INTRO_MS)
		return {
			phase: "intro",
			state: null,
			sessions: demo.all.map((c) => sprite(c, "idle", demo.startedAt)),
			nextAt: demo.startedAt + INTRO_MS,
		};
	const i = Math.floor((t - INTRO_MS) / STATE_MS);
	const state = DEMO_STATES[i];
	if (state === undefined)
		return {
			phase: "over",
			state: null,
			sessions: [],
			nextAt: Number.POSITIVE_INFINITY,
		};
	const since = demo.startedAt + INTRO_MS + i * STATE_MS;
	return {
		phase: "states",
		state,
		sessions: demo.picked.map((c) => sprite(c, state, since)),
		nextAt: since + STATE_MS,
	};
}
