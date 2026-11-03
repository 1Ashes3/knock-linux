// Character controller (SPEC.md §10.1, §8.7, ADRs 0016, 0017, 0031). Picks what each
// animal plays from the session's `state`, `tier` and `needs.seen` only: the core is the
// only source of truth (§7.3). Pure: time is always passed in.
import type { Clip } from "../../gallery/format.ts";
import type { SessionView } from "../bridge";

export type ClipName =
	| "sleep"
	| "work"
	| "wait"
	| "step_up"
	| "step_down"
	| "knock"
	| "alert"
	| "pace"
	| "present"
	| "rest";

export type Clips = Record<ClipName, Clip>;

/** `step_up` ends this many viewBox units up; `knock` is drawn in place (art/README.md). */
export const LIFT_UNITS = 12;

/**
 * One step of a plan.
 * - `once`: plays the clip once, then the next segment starts.
 * - `loop`: repeats until the state changes (Needs-you clips only, ADR 0031).
 * - `hold`: holds the clip's first frame, or the rig pose when `clip` is null. Settled.
 */
export interface Segment {
	clip: ClipName | null;
	play: "once" | "loop" | "hold";
	/** Extra lift in viewBox units, up positive. */
	lift: number;
}

export interface Plan {
	/** What the plan was made for; the same key keeps the running plan. */
	key: string;
	still: boolean;
	/** ms, same clock as `now` everywhere in this module. */
	start: number;
	segments: Segment[];
}

const once = (clip: ClipName, lift = 0): Segment => ({
	clip,
	play: "once",
	lift,
});
const loop = (clip: ClipName, lift = 0): Segment => ({
	clip,
	play: "loop",
	lift,
});
const hold = (clip: ClipName | null, lift = 0): Segment => ({
	clip,
	play: "hold",
	lift,
});

/** Which look a session gets. Only `state` and `needs.seen` matter. */
export function poseKey(s: SessionView): string {
	if (s.state === "needs_input")
		return s.needs?.seen ? "needs_seen" : "needs_unseen";
	return s.state;
}

/** Calm clips: one cycle, then hold the first frame (ADR 0031). */
const CALM: Partial<Record<string, ClipName>> = {
	working: "work",
	idle: "sleep",
	// Unused in v1 (§8.4); lead decision: `wait`, settled.
	waiting_agent: "wait",
	communicating: "wait",
};

/** Needs-you clips that keep looping until the state changes. */
const LOOPING: Partial<Record<string, ClipName>> = {
	failed: "alert",
	stalled: "pace",
};

/** The segments for a key, with Windows "Animation effects" on (`still` false) or off. */
export function segmentsFor(key: string, still: boolean): Segment[] {
	const calm = CALM[key];
	const looping = LOOPING[key];
	if (still) {
		// §10.1: no stepping or pacing; switch poses in place.
		if (key === "needs_unseen") return [hold("knock")];
		if (key === "ready") return [hold("rest")];
		if (calm) return [hold(calm)];
		if (looping) return [hold(looping)];
		return [hold(null)];
	}
	if (key === "needs_unseen")
		return [once("step_up"), loop("knock", LIFT_UNITS)];
	if (key === "ready") return [once("present"), once("rest"), hold("rest")];
	if (calm) return [once(calm), hold(calm)];
	if (looping) return [loop(looping)];
	// needs_seen (ADR 0016), ended_unexpectedly, blocked_fixable, paused, cancelled
	// and anything unknown: the rig pose, in place.
	return [hold(null)];
}

/**
 * The plan after a session update. The same look keeps its running plan, so a repeated
 * `sessions-changed` restarts nothing. Leaving a raised pose plays `step_down` first.
 */
export function nextPlan(
	prev: Plan | undefined,
	s: SessionView,
	still: boolean,
	now: number,
	clips: Clips,
): Plan {
	const key = poseKey(s);
	if (prev && prev.key === key && prev.still === still) return prev;
	const segments = segmentsFor(key, still);
	if (!still && prev && isRaised(prev, now, clips) && !raisedAtStart(segments))
		segments.unshift(once("step_down"));
	return { key, still, start: now, segments };
}

const raisedAtStart = (segs: Segment[]) =>
	segs[0]?.clip === "step_up" || (segs[0]?.lift ?? 0) > 0;

/** True while the animal is up out of the strip (stepping up or knocking). */
export function isRaised(plan: Plan, now: number, clips: Clips): boolean {
	const { segment } = current(plan, now, clips);
	return segment.lift > 0 || segment.clip === "step_up";
}

/** Whether the plan ends up raised: where the character's hit region goes (§9.3). */
export function endsRaised(plan: Plan): boolean {
	const last = plan.segments[plan.segments.length - 1];
	return (last?.lift ?? 0) > 0;
}

interface Current {
	segment: Segment;
	/** ms into the segment's clip. */
	t: number;
	/** When the next segment starts; Infinity for the last one. */
	endsAt: number;
}

function current(plan: Plan, now: number, clips: Clips): Current {
	let start = plan.start;
	const last = plan.segments.length - 1;
	for (let i = 0; i <= last; i++) {
		const segment = plan.segments[i] as Segment;
		if (segment.play === "hold" || !segment.clip)
			return { segment, t: 0, endsAt: Number.POSITIVE_INFINITY };
		const d = clips[segment.clip].durationMs;
		const elapsed = Math.max(0, now - start);
		if (segment.play === "loop")
			return { segment, t: elapsed % d, endsAt: Number.POSITIVE_INFINITY };
		if (elapsed < d || i === last)
			return { segment, t: Math.min(elapsed, d), endsAt: start + d };
		start += d;
	}
	throw new Error("plan has no segments");
}

export interface Frame {
	/** null: the rig pose. */
	clip: ClipName | null;
	t: number;
	lift: number;
	/** Nothing will move until the next state change. */
	settled: boolean;
}

/** What to draw at `now`. */
export function frameAt(plan: Plan, now: number, clips: Clips): Frame {
	const { segment, t } = current(plan, now, clips);
	return {
		clip: segment.clip,
		t,
		lift: segment.lift,
		settled: segment.play === "hold",
	};
}

/** When the plan settles: `start` for a still plan, Infinity for one that loops. */
export function settlesAt(plan: Plan, clips: Clips): number {
	let at = plan.start;
	for (const seg of plan.segments) {
		if (seg.play === "hold" || !seg.clip) return at;
		if (seg.play === "loop") return Number.POSITIVE_INFINITY;
		at += clips[seg.clip].durationMs;
	}
	return at;
}

/** How long a character takes to fade out (ended, or gone from the list). */
export const FADE_MS = 600;

/** §8.4, §10.1: an ended session fades out this long after `since`. */
export const ENDED_FADE_AFTER_MS = 60_000;

/** When an ended session starts to fade; null for any other state. */
export function endedFadeStart(s: SessionView): number | null {
	if (s.state !== "ended_unexpectedly") return null;
	const since = Date.parse(s.since);
	return Number.isFinite(since) ? since + ENDED_FADE_AFTER_MS : null;
}

/**
 * Opacity of a fade that starts at `from`. With "Animation effects" off a fade is a cut.
 * Before `from` it is 1; after `from + FADE_MS` it is 0.
 */
export function fadeAlpha(from: number, now: number, still: boolean): number {
	if (now < from) return 1;
	if (still) return 0;
	return Math.max(0, 1 - (now - from) / FADE_MS);
}

/** A fade is moving between `from` and `from + FADE_MS`, never with effects off. */
export function fading(from: number, now: number, still: boolean): boolean {
	return !still && now >= from && now < from + FADE_MS;
}

/** One drawn character, as the render loop sees it. */
export interface Animated {
	plan: Plan;
	/** When it starts fading out, or null. */
	fadeFrom: number | null;
}

/** §6 CPU: the render loop runs only while this is true. */
export function anyAnimating(
	chars: Iterable<Animated>,
	now: number,
	clips: Clips,
): boolean {
	for (const c of chars) {
		if (c.fadeFrom !== null && fading(c.fadeFrom, now, c.plan.still))
			return true;
		if (c.fadeFrom !== null && now >= c.fadeFrom + FADE_MS) continue; // gone
		if (!frameAt(c.plan, now, clips).settled) return true;
	}
	return false;
}
