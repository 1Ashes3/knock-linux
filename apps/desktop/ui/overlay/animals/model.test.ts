import { expect, test } from "vitest";
import type { SessionView } from "../bridge";
import { SAMPLES } from "../dev/samples";
import { CLIPS } from "./art";
import { ENDED_FADE_AFTER_MS, FADE_MS, settlesAt } from "./controller";
import { EMPTY_MEMORY, type Memory, step } from "./model";

const T0 = Date.parse("2026-11-01T12:00:00Z");
const run = (
	sessions: SessionView[],
	memory: Memory = EMPTY_MEMORY,
	now = T0,
	still = false,
) => step(sessions, memory, still, now, CLIPS);
const ids = (r: ReturnType<typeof run>) =>
	r.model.characters.map(
		(c) => `${c.slot}:${c.session.id}${c.leaving ? "~" : ""}`,
	);
const many = (n: number, s: SessionView = SAMPLES.working) =>
	Array.from({ length: n }, (_, i) => ({ ...s, id: `${s.id}${i}` }));

test("one character per session, first nearest the clock", () => {
	const r = run([SAMPLES.permission, SAMPLES.ready, SAMPLES.idle]);
	expect(ids(r)).toEqual(["0:permission", "1:ready", "2:idle"]);
	expect(r.model).toMatchObject({ hidden: [], slots: 3 });
});

test("above 5 sessions: 5 characters plus the +N slot, urgent tiers keep their character", () => {
	const idle = many(4, SAMPLES.idle);
	const r = run([...idle, SAMPLES.permission, SAMPLES.ready, SAMPLES.working]);
	expect(r.model.characters.map((c) => c.session.id)).toEqual([
		"idle0",
		"idle1",
		"permission",
		"ready",
		"working",
	]);
	expect(r.model.hidden.map((s) => s.id)).toEqual(["idle2", "idle3"]);
	expect(r.model.slots).toBe(6);
});

test("plans carry over between updates; a state change starts a new one", () => {
	const first = run([SAMPLES.working]);
	const again = run([{ ...SAMPLES.working }], first.memory, T0 + 5000);
	expect(again.model.characters[0]?.plan).toBe(first.model.characters[0]?.plan);
	const changed = run(
		[{ ...SAMPLES.working, state: "ready", tier: "ready" }],
		again.memory,
		T0 + 6000,
	);
	expect(changed.model.characters[0]?.plan.start).toBe(T0 + 6000);
	expect(changed.model.characters[0]?.plan.segments[0]?.clip).toBe("present");
});

test("wakes up when a calm animal settles, and not for loops", () => {
	const r = run([SAMPLES.working, SAMPLES.permission]);
	expect(r.model.wakeAt).toBe(T0 + CLIPS.work.durationMs);
	const later = run(
		[SAMPLES.working, SAMPLES.permission],
		r.memory,
		r.model.wakeAt,
	);
	expect(later.model.wakeAt).toBe(Number.POSITIVE_INFINITY);
});

test("an ended session fades 60 s after since, then leaves the strip", () => {
	const since = T0 - 10_000;
	const ended = { ...SAMPLES.ended, since: new Date(since).toISOString() };
	const fadeFrom = since + ENDED_FADE_AFTER_MS;
	const a = run([ended, SAMPLES.idle]);
	expect(a.model.characters[0]?.fadeFrom).toBe(fadeFrom);
	expect(a.model.wakeAt).toBe(T0 + CLIPS.sleep.durationMs);
	const settled = run([ended, SAMPLES.idle], a.memory, T0 + 10_000);
	expect(settled.model.wakeAt).toBe(fadeFrom);
	const fading = run([ended, SAMPLES.idle], settled.memory, fadeFrom + 1);
	expect(ids(fading)).toEqual(["0:ended", "1:idle"]);
	expect(fading.model.wakeAt).toBe(fadeFrom + FADE_MS);
	const gone = run([ended, SAMPLES.idle], fading.memory, fadeFrom + FADE_MS);
	expect(ids(gone)).toEqual(["0:idle"]);
	// Effects off: gone at once, with no fade.
	expect(ids(run([ended, SAMPLES.idle], EMPTY_MEMORY, fadeFrom, true))).toEqual(
		["0:idle"],
	);
});

test("a session that leaves the list fades out in its old slot, then the strip closes up", () => {
	const a = run([SAMPLES.permission, SAMPLES.ready, SAMPLES.idle]);
	const b = run([SAMPLES.permission, SAMPLES.idle], a.memory, T0 + 100);
	expect(ids(b)).toEqual(["0:permission", "1:ready~", "2:idle"]);
	const ghost = b.model.characters[1];
	expect(ghost?.fadeFrom).toBe(T0 + 100);
	expect(ghost?.plan).toBe(a.model.characters[1]?.plan); // keeps its pose
	expect(b.model.wakeAt).toBeLessThanOrEqual(T0 + 100 + FADE_MS);
	// An update during the fade keeps it where it is.
	const c = run(
		[SAMPLES.permission, SAMPLES.idle],
		b.memory,
		T0 + 100 + FADE_MS - 1,
	);
	expect(ids(c)).toEqual(["0:permission", "1:ready~", "2:idle"]);
	const d = run(
		[SAMPLES.permission, SAMPLES.idle],
		c.memory,
		T0 + 100 + FADE_MS,
	);
	expect(ids(d)).toEqual(["0:permission", "1:idle"]);
});

test("with effects off a session that leaves is gone at once", () => {
	const a = run([SAMPLES.ready, SAMPLES.idle], EMPTY_MEMORY, T0, true);
	const b = run([SAMPLES.idle], a.memory, T0 + 100, true);
	expect(ids(b)).toEqual(["0:idle"]);
});

test("a session that comes back during its fade is a normal character again", () => {
	const a = run([SAMPLES.ready, SAMPLES.idle]);
	const b = run([SAMPLES.idle], a.memory, T0 + 100);
	const c = run([SAMPLES.ready, SAMPLES.idle], b.memory, T0 + 200);
	expect(ids(c)).toEqual(["0:ready", "1:idle"]);
});

test("a leaving character never pushes the strip past 5 characters", () => {
	const five = many(5);
	const a = run(five);
	const b = run([...five.slice(1), SAMPLES.idle], a.memory, T0 + 100);
	expect(b.model.characters).toHaveLength(5);
	expect(b.model.characters.every((c) => !c.leaving)).toBe(true);
});

test("moving into +N is not leaving", () => {
	const five = many(5, SAMPLES.idle);
	const a = run(five);
	const b = run([...five, SAMPLES.permission], a.memory, T0 + 100);
	expect(b.model.characters.some((c) => c.leaving)).toBe(false);
	expect(b.model.hidden.map((s) => s.id)).toEqual(["idle4"]);
});

test("every plan settles or loops: nothing is left half-way", () => {
	const r = run(Object.values(SAMPLES).slice(0, 5));
	for (const c of r.model.characters)
		expect(settlesAt(c.plan, CLIPS)).toBeGreaterThanOrEqual(T0);
});
