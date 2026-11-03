import { expect, test } from "vitest";
import { ART_ROOT, type ArtFiles, checkArt, readArt } from "./artCheck.ts";

// Each broken fixture is the real art with one thing broken, built in memory.
const real = readArt(ART_ROOT);
const broken = (edit: (f: ArtFiles) => void): string[] => {
	const files = new Map(real);
	edit(files);
	return checkArt(files);
};
// biome-ignore lint/suspicious/noExplicitAny: tests break the JSON on purpose, so it has no fixed shape.
const json = (f: ArtFiles, path: string, edit: (v: any) => void) => {
	const v = JSON.parse(f.get(path) as string);
	edit(v);
	f.set(path, JSON.stringify(v));
};
const RIG = "animals/fox/rig.json";

test("the real art passes", () => {
	expect(checkArt(real)).toEqual([]);
});

test("a missing required part fails", () => {
	const errors = broken((f) =>
		json(f, RIG, (r) => {
			r.parts = r.parts.filter((p: { name: string }) => p.name !== "ear_l");
		}),
	);
	expect(errors).toContain("fox: missing required part ear_l");
});

test("a missing or malformed pivot fails", () => {
	const errors = broken((f) =>
		json(f, RIG, (r) => {
			delete r.parts[0].pivot;
			r.parts[1].pivot = [1, "2"];
		}),
	);
	expect(errors.filter((e) => e.includes("pivot must be [x, y]"))).toHaveLength(
		2,
	);
});

test("a missing part file fails", () => {
	const errors = broken((f) => f.delete("animals/fox/tail.svg"));
	expect(errors).toContain("fox/tail: file tail.svg is missing");
});

test("a part file that is not SVG fails", () => {
	const errors = broken((f) => f.set("animals/fox/tail.svg", "hello"));
	expect(errors).toContain("fox/tail: tail.svg is not an SVG file");
	const png = broken((f) =>
		json(f, RIG, (r) => (r.parts[0].file = "tail.png")),
	);
	expect(png).toContain("fox/tail: file must be an .svg file");
});

test("scripts and gradients are rejected", () => {
	const errors = broken((f) =>
		f.set(
			"animals/fox/tail.svg",
			'<svg viewBox="0 0 64 64"><script>alert(1)</script></svg>',
		),
	);
	expect(errors.some((e) => e.includes("must be flat shapes only"))).toBe(true);
});

test("more than 6 colors per animal fails, counted across all parts", () => {
	const errors = broken((f) =>
		f.set(
			"animals/fox/tail.svg",
			'<svg viewBox="0 0 64 64"><path fill="#123" stroke="#00ff00"/><path style="fill:red"/></svg>',
		),
	);
	expect(errors.some((e) => /^fox: uses 7 colors/.test(e))).toBe(true);
});

test("a missing required clip fails", () => {
	const errors = broken((f) => f.delete("clips/knock.json"));
	expect(errors).toContain("missing required clip knock");
});

test("a track on an unknown part fails", () => {
	const errors = broken((f) =>
		json(f, "clips/work.json", (c) => {
			c.tracks.whiskers = [{ t: 0, x: 1 }];
		}),
	);
	// The "not in" list names every animal, in folder-read order.
	expect(errors).toContainEqual(
		expect.stringMatching(
			/^clips\/work\.json: track whiskers: unknown part \(not in .*fox/,
		),
	);
});

test("an unknown track field fails", () => {
	const errors = broken((f) =>
		json(f, "clips/work.json", (c) => {
			c.tracks.head[0].skew = 3;
		}),
	);
	expect(errors).toContain(
		"clips/work.json: track head: unknown track field skew",
	);
});

test("keyframes out of order or outside the clip fail", () => {
	const errors = broken((f) =>
		json(f, "clips/work.json", (c) => {
			c.tracks.paw_l = [
				{ t: 200, y: 1 },
				{ t: 100, y: 0 },
			];
			c.tracks.paw_r = [{ t: 5000, y: 1 }];
		}),
	);
	expect(errors).toContain(
		"clips/work.json: track paw_l: keyframes must be in increasing t order",
	);
	expect(errors).toContain(
		"clips/work.json: track paw_r: every keyframe needs t between 0 and 800",
	);
});

test("a swap that names an unknown part fails", () => {
	const errors = broken((f) =>
		json(f, "clips/sleep.json", (c) => {
			c.swaps[0].to = "eyes_shut";
		}),
	);
	expect(errors).toContainEqual(
		expect.stringMatching(
			/^clips\/sleep\.json: swap names unknown part eyes_shut \(not in .*fox/,
		),
	);
});

test("a clip whose name does not match its file fails", () => {
	const errors = broken((f) =>
		json(f, "clips/rest.json", (c) => {
			c.name = "nap";
		}),
	);
	expect(errors).toContain('clips/rest.json: name must be "rest"');
});
