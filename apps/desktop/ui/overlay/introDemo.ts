// "Introduce sprites" (ADR 0040): a short demo of the selected pack's characters, drawn in
// place of the real sprites. Pure: the demo is a value, and what it shows at a time is a
// function of that value and the time, so every step can be tested.
//
// The demo never touches real session state. Its sprites are made up here, are local to
// the overlay, and never reach the core: no jump target, no dismiss, no notification.
import type { SessionView, State, Tier } from "./bridge";

/** The introduction: every character, asleep. */
export const INTRO_MS = 3000;
/** The whole demo when the `intro-demo` event names no length (§8.5: the core sends 18 000). */
export const DEMO_MS = 18_000;
/** How many characters go through the states. */
export const PICKED = 4;

/**
 * The states the picked characters go through (ADR 0040, amended). In each round every
 * picked character shows a different one of them, and they rotate each round.
 */
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
	/** The whole demo, introduction included, from the `intro-demo` event. */
	durationMs: number;
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
	/** Which round of states it is (0 to 4); null outside "states". */
	round: number | null;
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

/**
 * The demo's length from the event payload: `durationMs` if it is a usable number (long
 * enough for the introduction and the rounds), otherwise DEMO_MS.
 */
export function demoDuration(payload: unknown): number {
	const d =
		payload && typeof payload === "object" && "durationMs" in payload
			? (payload as { durationMs: unknown }).durationMs
			: undefined;
	return typeof d === "number" && Number.isFinite(d) && d > INTRO_MS
		? d
		: DEMO_MS;
}

/** Starts a demo at `now` with the pack's characters; null if the pack has none. */
export function startDemo(
	chars: readonly DemoCharacter[],
	now: number,
	real: readonly SessionView[],
	random: () => number = Math.random,
	durationMs: number = DEMO_MS,
): Demo | null {
	if (!chars.length) return null;
	return {
		startedAt: now,
		durationMs: durationMs > INTRO_MS ? durationMs : DEMO_MS,
		all: [...chars],
		picked: pick(chars, PICKED, random),
		urgentAtStart: urgentKeys(real),
	};
}

/** When the demo is over. */
export function demoEnd(demo: Demo): number {
	return demo.startedAt + demo.durationMs;
}

/** How long each round of states lasts: what is left after the introduction, in 5. */
export function roundMs(demo: Demo): number {
	return (demo.durationMs - INTRO_MS) / DEMO_STATES.length;
}

/** The state picked character `k` shows in round `round`: all different within a round. */
export function stateOf(k: number, round: number): DemoState {
	return DEMO_STATES[(k + round) % DEMO_STATES.length] as DemoState;
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
			round: null,
			sessions: demo.all.map((c) => sprite(c, "idle", demo.startedAt)),
			nextAt: demo.startedAt + INTRO_MS,
		};
	const round = roundMs(demo);
	const i = Math.floor((t - INTRO_MS) / round);
	if (t >= demo.durationMs || i >= DEMO_STATES.length)
		return {
			phase: "over",
			round: null,
			sessions: [],
			nextAt: Number.POSITIVE_INFINITY,
		};
	const since = demo.startedAt + INTRO_MS + i * round;
	return {
		phase: "states",
		round: i,
		sessions: demo.picked.map((c, k) => sprite(c, stateOf(k, i), since)),
		nextAt: i === DEMO_STATES.length - 1 ? demoEnd(demo) : since + round,
	};
}
