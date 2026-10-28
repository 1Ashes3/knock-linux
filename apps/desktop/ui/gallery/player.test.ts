import { expect, test } from "vitest";
import type { Clip } from "./format.ts";
import { clipTime, partTransform, poseAt, sample } from "./player.ts";

const clip = (over: Partial<Clip> = {}): Clip => ({
	name: "c",
	loop: true,
	durationMs: 1000,
	tracks: {},
	swaps: [],
	...over,
});

test("looping clips wrap, once clips clamp and hold", () => {
	expect(clipTime(clip(), 2500)).toBe(500);
	expect(clipTime(clip(), 1000)).toBe(0);
	expect(clipTime(clip({ loop: false }), 2500)).toBe(1000);
	expect(clipTime(clip({ loop: false }), -5)).toBe(0);
});

test("sample eases between keys and holds outside them", () => {
	const keys = [
		{ t: 100, y: 0 },
		{ t: 300, y: -10 },
	];
	expect(sample(keys, "y", 0)).toBe(0);
	expect(sample(keys, "y", 100)).toBe(0);
	expect(sample(keys, "y", 200)).toBeCloseTo(-5); // midpoint of sine ease
	expect(sample(keys, "y", 150)).toBeGreaterThan(-2.5); // eases in, slower than linear
	expect(sample(keys, "y", 300)).toBe(-10);
	expect(sample(keys, "y", 900)).toBe(-10);
});

test("each field uses only the keys that set it; unset fields stay at the rig pose", () => {
	const keys = [
		{ t: 0, x: 2 },
		{ t: 500, y: 4 },
		{ t: 1000, x: 6 },
	];
	expect(sample(keys, "x", 500)).toBeCloseTo(4);
	expect(sample(keys, "y", 0)).toBe(4);
	expect(sample(keys, "rot", 500)).toBe(0);
	expect(sample(keys, "scaleX", 500)).toBe(1);
	expect(sample(keys, "alpha", 500)).toBe(1);
});

test("eyes_closed is hidden at rest and a swap shows it only inside [start, end)", () => {
	const c = clip({
		swaps: [{ from: "eyes_open", to: "eyes_closed", start: 200, end: 400 }],
	});
	const parts = ["eyes_open", "eyes_closed", "head"];
	const vis = (t: number) => {
		const p = poseAt(c, parts, t);
		return [p.eyes_open?.visible, p.eyes_closed?.visible, p.head?.visible];
	};
	expect(vis(199)).toEqual([true, false, true]);
	expect(vis(200)).toEqual([false, true, true]);
	expect(vis(399)).toEqual([false, true, true]);
	expect(vis(400)).toEqual([true, false, true]);
});

test("partTransform offsets, then rotates and scales around the pivot", () => {
	const pose = poseAt(
		clip({ tracks: { head: [{ t: 0, x: 1, y: -2, rot: 10, scaleY: 1.5 }] } }),
		["head"],
		0,
	).head;
	expect(pose && partTransform([32, 34], pose)).toBe(
		"translate(33 32) rotate(10) scale(1 1.5) translate(-32 -34)",
	);
});
