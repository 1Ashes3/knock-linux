import { expect, test } from "vitest";
import type { SessionView, State } from "../bridge";
import { SAMPLES } from "../dev/samples";
import { CLIPS, FALLBACK_ANIMAL, RIGS, rigFor } from "./art";
import {
	anyAnimating,
	ENDED_FADE_AFTER_MS,
	endedFadeStart,
	endsRaised,
	FADE_MS,
	fadeAlpha,
	frameAt,
	LIFT_UNITS,
	nextPlan,
	type Plan,
	poseKey,
	segmentsFor,
	settlesAt,
} from "./controller";

const T0 = 1_000_000;
const plan = (s: SessionView, still = false, prev?: Plan, now = T0) =>
	nextPlan(prev, s, still, now, CLIPS);
const frame = (p: Plan, at: number) => frameAt(p, at, CLIPS);
const as = (state: State, tier: SessionView["tier"]): SessionView => ({
	...SAMPLES.working,
	state,
	tier,
});

/** [session, segments as "clip:play:lift" with effects on, then off] */
const CASES: [string, SessionView, string[], string[]][] = [
	[
		"needs permission",
		SAMPLES.permission,
		["step_up:once:0", "knock:loop:12"],
		["knock:hold:0"],
	],
	[
		"needs permission, no tool",
		SAMPLES.permission_no_tool,
		["step_up:once:0", "knock:loop:12"],
		["knock:hold:0"],
	],
	[
		"question",
		SAMPLES.question,
		["step_up:once:0", "knock:loop:12"],
		["knock:hold:0"],
	],
	["seen (ADR 0016)", SAMPLES.seen, ["rig:hold:0"], ["rig:hold:0"]],
	["failed", SAMPLES.failed, ["alert:loop:0"], ["alert:hold:0"]],
	["stalled", SAMPLES.stalled, ["pace:loop:0"], ["pace:hold:0"]],
	[
		"ready",
		SAMPLES.ready,
		["present:once:0", "rest:once:0", "rest:hold:0"],
		["rest:hold:0"],
	],
	["working", SAMPLES.working, ["work:once:0", "work:hold:0"], ["work:hold:0"]],
	["idle", SAMPLES.idle, ["sleep:once:0", "sleep:hold:0"], ["sleep:hold:0"]],
	["ended", SAMPLES.ended, ["rig:hold:0"], ["rig:hold:0"]],
	// Unused in v1 (§8.4): lead fallbacks, and none of them crashes.
	[
		"waiting_agent",
		as("waiting_agent", "working"),
		["wait:once:0", "wait:hold:0"],
		["wait:hold:0"],
	],
	[
		"communicating",
		as("communicating", "working"),
		["wait:once:0", "wait:hold:0"],
		["wait:hold:0"],
	],
	[
		"blocked_fixable",
		as("blocked_fixable", "needs_you"),
		["rig:hold:0"],
		["rig:hold:0"],
	],
	["paused", as("paused", "resting"), ["rig:hold:0"], ["rig:hold:0"]],
	["cancelled", as("cancelled", "resting"), ["rig:hold:0"], ["rig:hold:0"]],
	[
		"a state this UI does not know",
		as("something_new" as State, "resting"),
		["rig:hold:0"],
		["rig:hold:0"],
	],
];

const describe = (p: Plan) =>
	p.segments.map((s) => `${s.clip ?? "rig"}:${s.play}:${s.lift}`);

test.each(CASES)("clip choice: %s", (_, s, on, off) => {
	expect(describe(plan(s))).toEqual(on);
	expect(describe(plan(s, true))).toEqual(off);
});

test("a needs_input without needs is treated as not seen", () => {
	expect(poseKey({ ...SAMPLES.permission, needs: null })).toBe("needs_unseen");
});

test("Needs you: step_up, then knock loops 12 units up until the state changes", () => {
	const p = plan(SAMPLES.permission);
	const up = CLIPS.step_up.durationMs;
	const knock = CLIPS.knock.durationMs;
	expect(frame(p, T0)).toEqual({
		clip: "step_up",
		t: 0,
		lift: 0,
		settled: false,
	});
	expect(frame(p, T0 + up - 1)).toMatchObject({ clip: "step_up", lift: 0 });
	expect(frame(p, T0 + up)).toMatchObject({
		clip: "knock",
		t: 0,
		lift: LIFT_UNITS,
	});
	expect(frame(p, T0 + up + knock * 50 + 120)).toEqual({
		clip: "knock",
		t: 120,
		lift: LIFT_UNITS,
		settled: false,
	});
	expect(settlesAt(p, CLIPS)).toBe(Number.POSITIVE_INFINITY);
	expect(endsRaised(p)).toBe(true);
});

test("Ready: present once, then one cycle of rest, then hold rest's first frame", () => {
	const p = plan(SAMPLES.ready);
	const present = CLIPS.present.durationMs;
	const rest = CLIPS.rest.durationMs;
	expect(frame(p, T0 + present - 1)).toMatchObject({ clip: "present" });
	expect(frame(p, T0 + present)).toMatchObject({
		clip: "rest",
		t: 0,
		settled: false,
	});
	expect(frame(p, T0 + present + rest - 1)).toMatchObject({
		clip: "rest",
		settled: false,
	});
	expect(frame(p, T0 + present + rest)).toEqual({
		clip: "rest",
		t: 0,
		lift: 0,
		settled: true,
	});
	expect(settlesAt(p, CLIPS)).toBe(T0 + present + rest);
});

test.each([
	["working", SAMPLES.working, "work"],
	["idle", SAMPLES.idle, "sleep"],
	["waiting_agent", as("waiting_agent", "working"), "wait"],
] as const)(
	"calm %s plays one cycle of %s, then holds its first frame (ADR 0031)",
	(_, s, clip) => {
		const p = plan(s);
		const d = CLIPS[clip].durationMs;
		expect(frame(p, T0 + d / 2)).toEqual({
			clip,
			t: d / 2,
			lift: 0,
			settled: false,
		});
		expect(frame(p, T0 + d)).toEqual({ clip, t: 0, lift: 0, settled: true });
		expect(frame(p, T0 + d * 100)).toEqual({
			clip,
			t: 0,
			lift: 0,
			settled: true,
		});
		expect(settlesAt(p, CLIPS)).toBe(T0 + d);
	},
);

test("Needs-you clips keep looping: alert and pace never settle", () => {
	for (const s of [SAMPLES.failed, SAMPLES.stalled]) {
		const p = plan(s);
		expect(frame(p, T0 + 3_600_000).settled).toBe(false);
		expect(settlesAt(p, CLIPS)).toBe(Number.POSITIVE_INFINITY);
	}
});

test("a repeated update with the same look keeps the running plan", () => {
	const p = plan(SAMPLES.permission);
	const later = T0 + 5000;
	const other = {
		...SAMPLES.permission,
		needs: { kind: "permission", label: "Bash", seen: false },
		since: new Date().toISOString(),
	} as SessionView;
	expect(plan(other, false, p, later)).toBe(p);
	expect(plan(SAMPLES.working, false, plan(SAMPLES.working), later).start).toBe(
		T0,
	);
});

test("leaving Needs you after a step-up plays step_down first", () => {
	const up = plan(SAMPLES.permission);
	const at = T0 + 2000; // knocking, raised
	const down = CLIPS.step_down.durationMs;
	for (const [s, then] of [
		[SAMPLES.working, "work"],
		[SAMPLES.ready, "present"],
		[SAMPLES.failed, "alert"],
		[SAMPLES.seen, null],
	] as const) {
		const p = plan(s, false, up, at);
		expect(p.segments[0]).toEqual({ clip: "step_down", play: "once", lift: 0 });
		expect(frame(p, at)).toMatchObject({ clip: "step_down", t: 0 });
		expect(frame(p, at + down)).toMatchObject({ clip: then, t: 0 });
		expect(endsRaised(p)).toBe(false);
	}
});

test("leaving mid step_up also steps down; calm changes never do", () => {
	const up = plan(SAMPLES.question);
	expect(plan(SAMPLES.working, false, up, T0 + 100).segments[0]?.clip).toBe(
		"step_down",
	);
	const working = plan(SAMPLES.working);
	expect(plan(SAMPLES.ready, false, working, T0 + 100).segments[0]?.clip).toBe(
		"present",
	);
	const failed = plan(SAMPLES.failed);
	expect(plan(SAMPLES.working, false, failed, T0 + 100).segments[0]?.clip).toBe(
		"work",
	);
});

test("a new unseen episode after seen steps up again", () => {
	const seen = plan(SAMPLES.seen);
	const p = plan(SAMPLES.permission, false, seen, T0 + 9000);
	expect(describe(p)).toEqual(["step_up:once:0", "knock:loop:12"]);
	expect(p.start).toBe(T0 + 9000);
});

test("Animation effects off: every state is a still pose in place, settled at once", () => {
	for (const [, s] of CASES) {
		const p = plan(s, true);
		expect(frame(p, T0)).toMatchObject({ t: 0, lift: 0, settled: true });
		expect(endsRaised(p)).toBe(false);
		expect(settlesAt(p, CLIPS)).toBe(T0);
	}
	// No step_down either: a knocking animal just switches pose.
	const up = plan(SAMPLES.permission);
	expect(describe(plan(SAMPLES.working, true, up, T0 + 2000))).toEqual([
		"work:hold:0",
	]);
});

test("turning Animation effects off or on re-plans in place", () => {
	const on = plan(SAMPLES.permission);
	const off = plan(SAMPLES.permission, true, on, T0 + 2000);
	expect(off).not.toBe(on);
	expect(off.still).toBe(true);
	expect(describe(off)).toEqual(["knock:hold:0"]);
});

test("an animal with no art falls back to the fox", () => {
	expect(RIGS.has(FALLBACK_ANIMAL)).toBe(true);
	expect(rigFor("unicorn").id).toBe("fox");
	expect(rigFor("").id).toBe("fox");
	expect(rigFor("fox").id).toBe("fox");
	for (const id of RIGS.keys()) expect(rigFor(id).id).toBe(id);
});

test("ended fades 60 s after since; a fade is a cut with effects off", () => {
	const since = Date.parse(SAMPLES.ended.since);
	const from = endedFadeStart(SAMPLES.ended);
	expect(from).toBe(since + ENDED_FADE_AFTER_MS);
	expect(endedFadeStart(SAMPLES.idle)).toBeNull();
	expect(endedFadeStart({ ...SAMPLES.ended, since: "not a time" })).toBeNull();
	expect(fadeAlpha(1000, 999, false)).toBe(1);
	expect(fadeAlpha(1000, 1000 + FADE_MS / 2, false)).toBeCloseTo(0.5);
	expect(fadeAlpha(1000, 1000 + FADE_MS, false)).toBe(0);
	expect(fadeAlpha(1000, 1000, true)).toBe(0);
});

test("the render loop reports nothing animating once everything has settled", () => {
	const calm = [SAMPLES.working, SAMPLES.idle, SAMPLES.ready, SAMPLES.seen].map(
		(s) => ({ plan: plan(s), fadeFrom: null }),
	);
	expect(anyAnimating(calm, T0, CLIPS)).toBe(true);
	const settled = Math.max(...calm.map((c) => settlesAt(c.plan, CLIPS)));
	expect(anyAnimating(calm, settled - 1, CLIPS)).toBe(true);
	expect(anyAnimating(calm, settled, CLIPS)).toBe(false);
	expect(anyAnimating([], T0, CLIPS)).toBe(false);

	// A knock keeps it running; so does a fade, until it ends.
	const knock = { plan: plan(SAMPLES.permission), fadeFrom: null };
	expect(anyAnimating([...calm, knock], T0 + 600_000, CLIPS)).toBe(true);
	const fadeFrom = settled + 1000;
	const ended = { plan: plan(SAMPLES.ended), fadeFrom };
	expect(anyAnimating([...calm, ended], fadeFrom - 1, CLIPS)).toBe(false);
	expect(anyAnimating([...calm, ended], fadeFrom, CLIPS)).toBe(true);
	expect(anyAnimating([...calm, ended], fadeFrom + FADE_MS, CLIPS)).toBe(false);
	// A knocking session that has faded out (gone from the list) no longer counts.
	const gone = { plan: plan(SAMPLES.permission), fadeFrom: T0 };
	expect(anyAnimating([gone], T0 + FADE_MS, CLIPS)).toBe(false);
});

test("with effects off nothing ever animates", () => {
	const all = CASES.map(([, s]) => ({ plan: plan(s, true), fadeFrom: null }));
	expect(anyAnimating(all, T0, CLIPS)).toBe(false);
	const ended = { plan: plan(SAMPLES.ended, true), fadeFrom: T0 };
	expect(anyAnimating([ended], T0, CLIPS)).toBe(false);
});

test("segmentsFor returns fresh arrays (plans never share segments)", () => {
	const a = segmentsFor("ready", false);
	a.pop();
	expect(segmentsFor("ready", false)).toHaveLength(3);
});
