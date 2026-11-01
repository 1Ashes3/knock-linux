// What a plain-strip pill shows (SPEC.md §10.1, §10.3, §10.5). Pure: everything comes
// from the SessionView the core sent (§7.3).
import type { SessionView, Size, State, Tier } from "./bridge";

/** §9.2: strip height, and slot width, in logical px. */
export const SLOT_PX: Record<Size, number> = {
	small: 48,
	medium: 64,
	large: 80,
};

/** §9.2: 6 slots at most, so 5 sessions plus the "+N" pill (the den is Phase 4). */
export const MAX_VISIBLE = 5;

export const INK = "#1a1a1a";
export const RED = "#c62828";

/** Pill fill per tier. Text is always INK; contrast is tested in pill.test.ts. */
export const TIER_FILL: Record<Tier, string> = {
	needs_you: "#ffb547",
	ready: "#6fd58f",
	working: "#8cb8ff",
	resting: "#d3d7dd",
};

export const OVERFLOW_FILL = "#ffffff";

/** The full state label (§10.1 chip text, or a plain name where §10.1 has none). */
export function stateLabel(
	s: SessionView,
	now: number,
	stallMinutes: number,
): string {
	switch (s.state) {
		case "needs_input":
			if (!s.needs) return "Needs you";
			if (s.needs.seen) return "Waiting for you";
			if (s.needs.kind === "answer") return "Has a question";
			return s.needs.label
				? `Needs permission: ${s.needs.label}`
				: "Needs permission";
		case "stalled":
			return `No progress for ${noProgressMinutes(s, now, stallMinutes)} min`;
		case "failed":
			return "Stopped with an error";
		case "ready":
			return "Done, ready to review";
		case "ended_unexpectedly":
			return "Session ended";
		case "working":
		case "communicating":
			return "Working";
		case "waiting_agent":
			return "Waiting on another agent";
		case "blocked_fixable":
			return "Blocked";
		case "idle":
			return "Idle";
		case "paused":
			return "Paused";
		case "cancelled":
			return "Cancelled";
	}
}

/**
 * Stalled is entered after `stallMinutes` without progress, and `since` is when it was
 * entered, so the time without progress is the two added.
 * ponytail: assumes stallMinutes did not change since; a "last event" time in SessionView would make this exact.
 */
function noProgressMinutes(
	s: SessionView,
	now: number,
	stallMinutes: number,
): number {
	const inState = Math.floor((now - Date.parse(s.since)) / 60_000);
	return stallMinutes + (Number.isFinite(inState) ? Math.max(0, inState) : 0);
}

/** §10.5: what Narrator reads. */
export function accessibleName(
	s: SessionView,
	now: number,
	stallMinutes: number,
): string {
	return `${s.project}, ${stateLabel(s, now, stallMinutes)}`;
}

const WORD: Record<Exclude<State, "needs_input">, string> = {
	failed: "Error",
	stalled: "Stuck?",
	blocked_fixable: "Blocked",
	ready: "Done",
	working: "Busy",
	waiting_agent: "Waiting",
	communicating: "Busy",
	idle: "Idle",
	ended_unexpectedly: "Ended",
	paused: "Paused",
	cancelled: "Stopped",
};

/** The short status word on the pill (at most 7 characters, to fit a 48 px slot). */
export function shortWord(s: SessionView): string {
	if (s.state !== "needs_input") return WORD[s.state];
	if (!s.needs || s.needs.seen) return "Waiting";
	return s.needs.kind === "answer" ? "Reply?" : "Allow?";
}

export type Glyph = "ask" | "failed" | "check" | "dots" | "z";

const TIER_GLYPH: Record<Tier, Glyph> = {
	needs_you: "ask",
	ready: "check",
	working: "dots",
	resting: "z",
};

/** One glyph per tier, so color is never the only signal (§6). Failed adds the red marker. */
export function glyph(s: SessionView): Glyph {
	return s.state === "failed" ? "failed" : TIER_GLYPH[s.tier];
}

/** §8.5 `dismiss` applies to Failed and ended. */
export function dismissible(s: SessionView): boolean {
	return s.state === "failed" || s.state === "ended_unexpectedly";
}

/** §10.1: only an unseen Needs-input knocks. */
export function knocks(s: SessionView): boolean {
	return s.state === "needs_input" && s.needs?.seen === false;
}

const TIER_RANK: Record<Tier, number> = {
	needs_you: 0,
	ready: 1,
	working: 2,
	resting: 3,
};

/**
 * Up to 5 sessions get a pill, in the core's order; the rest go into "+N".
 * When some must be hidden, the more urgent tiers keep their pill.
 */
export function layout(sessions: SessionView[]): {
	visible: SessionView[];
	hidden: SessionView[];
} {
	if (sessions.length <= MAX_VISIBLE) return { visible: sessions, hidden: [] };
	const keep = new Set(
		sessions
			.map((s, i) => ({ s, i }))
			.sort((a, b) => TIER_RANK[a.s.tier] - TIER_RANK[b.s.tier] || a.i - b.i)
			.slice(0, MAX_VISIBLE)
			.map(({ s }) => s),
	);
	return {
		visible: sessions.filter((s) => keep.has(s)),
		hidden: sessions.filter((s) => !keep.has(s)),
	};
}

/** Narrator name of the "+N" pill. */
export function overflowName(hidden: SessionView[]): string {
	const needYou = hidden.filter((s) => s.tier === "needs_you").length;
	const more = `${hidden.length} more ${hidden.length === 1 ? "session" : "sessions"}`;
	return needYou
		? `${more}, ${needYou} ${needYou === 1 ? "needs" : "need"} you`
		: more;
}
