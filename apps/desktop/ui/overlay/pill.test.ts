import { expect, test } from "vitest";
import type { SessionView } from "./bridge";
import { EIGHT_DEN, SAMPLES } from "./dev/samples";
import {
	accessibleName,
	DEN_FILL,
	denCounts,
	denName,
	dismissible,
	glyph,
	INK,
	knocks,
	layout,
	RED,
	shortWord,
	TIER_FILL,
	TIERS,
	tierGlyph,
} from "./pill";

const NOW = Date.parse("2026-11-01T12:00:00Z");
const at = (minutesAgo: number) =>
	new Date(NOW - minutesAgo * 60_000).toISOString();

// Every v1 state and Needs-input variant (§10.1, ADR 0016):
// sample, accessible name, short word, glyph, knocks, dismissible.
const CASES: [SessionView, string, string, string, boolean, boolean][] = [
	[
		SAMPLES.permission,
		"knock, Needs permission: PowerShell",
		"Allow?",
		"ask",
		true,
		false,
	],
	[
		SAMPLES.permission_no_tool,
		"api, Needs permission",
		"Allow?",
		"ask",
		true,
		false,
	],
	[SAMPLES.question, "website, Has a question", "Reply?", "ask", true, false],
	[SAMPLES.seen, "notes, Waiting for you", "Waiting", "ask", false, false],
	[
		{ ...SAMPLES.question, needs: { kind: "answer", label: null, seen: true } },
		"website, Waiting for you",
		"Waiting",
		"ask",
		false,
		false,
	],
	[
		SAMPLES.failed,
		"billing-service, Stopped with an error",
		"Error",
		"failed",
		false,
		true,
	],
	[
		{ ...SAMPLES.stalled, since: at(3) },
		"data, No progress for 13 min",
		"Stuck?",
		"ask",
		false,
		false,
	],
	[SAMPLES.ready, "docs, Done, ready to review", "Done", "check", false, false],
	[SAMPLES.working, "mobile-app, Working", "Busy", "dots", false, false],
	[SAMPLES.idle, "scripts, Idle", "Idle", "z", false, false],
	[SAMPLES.ended, "infra, Session ended", "Ended", "z", false, true],
];

test.each(CASES)("%o", (s, name, word, g, knock, dismiss) => {
	expect(accessibleName(s, NOW, 10)).toBe(name);
	expect(shortWord(s)).toBe(word);
	expect(glyph(s)).toBe(g);
	expect(knocks(s)).toBe(knock);
	expect(dismissible(s)).toBe(dismiss);
	expect(word.length).toBeLessThanOrEqual(7);
});

test("stalled minutes: stallMinutes plus whole minutes in the state", () => {
	const s = { ...SAMPLES.stalled, since: at(0.5) };
	expect(accessibleName(s, NOW, 10)).toBe("data, No progress for 10 min");
	expect(accessibleName({ ...s, since: at(61) }, NOW, 5)).toBe(
		"data, No progress for 66 min",
	);
	expect(accessibleName({ ...s, agent: "copilot" }, NOW, 10)).toBe(
		"data (GitHub Copilot), No progress for 10 min",
	);
	expect(accessibleName({ ...s, since: "bad" }, NOW, 10)).toBe(
		"data, No progress for 10 min",
	);
});

test("every tier has its own glyph; failed adds the red marker", () => {
	const tiers = [
		SAMPLES.permission,
		SAMPLES.ready,
		SAMPLES.working,
		SAMPLES.idle,
	];
	expect(new Set(tiers.map(glyph)).size).toBe(4);
	expect(glyph(SAMPLES.failed)).not.toBe(glyph(SAMPLES.permission));
});

const many = (n: number, s: SessionView = SAMPLES.working) =>
	Array.from({ length: n }, (_, i) => ({ ...s, id: `${s.id}${i}` }));

test("up to 5 sessions all get a pill, in the core's order", () => {
	const sessions = many(5);
	expect(layout(sessions)).toEqual({ visible: sessions, hidden: [] });
});

const ids = (list: SessionView[]) => list.map((s) => s.id);
const needs = (s: SessionView): SessionView => ({
	...s,
	tier: "needs_you",
	state: "needs_input",
	needs: { kind: "permission", label: "Bash", seen: false },
});

test("above 5: Needs you first, then the core's order; slots keep the core's order", () => {
	const idle = many(3, SAMPLES.idle);
	const working = many(2);
	const sessions = [
		idle[0],
		working[0],
		SAMPLES.ready,
		idle[1],
		working[1],
		SAMPLES.permission,
		idle[2],
	] as SessionView[];
	const { visible, hidden } = layout(sessions);
	expect(ids(visible)).toEqual([
		"idle0",
		"working0",
		"ready",
		"idle1",
		"permission",
	]);
	expect(ids(hidden)).toEqual(["working1", "idle2"]);
	expect(denName(hidden)).toBe("2 more sessions: 1 working, 1 resting");
});

test("only Needs-you steps out of the den, and steps back in when it calms down", () => {
	const six = many(6);
	expect(ids(layout(six).hidden)).toEqual(["working5"]);
	// Ready, Working and Resting never move a den session into a slot.
	for (const calm of [SAMPLES.ready, SAMPLES.idle, SAMPLES.ended]) {
		const list = six.map((s, i) =>
			i === 5 ? { ...s, tier: calm.tier, state: calm.state } : s,
		);
		expect(ids(layout(list).hidden)).toEqual(["working5"]);
	}
	// Needs you: it takes the last calm slot, and that session goes into the den.
	const knocking = six.map((s, i) => (i === 5 ? needs(s) : s));
	const out = layout(knocking);
	expect(ids(out.visible)).toEqual([
		"working0",
		"working1",
		"working2",
		"working3",
		"working5",
	]);
	expect(ids(out.hidden)).toEqual(["working4"]);
	// Every Needs-you kind steps out, seen or not (ADR 0016).
	for (const s of [SAMPLES.failed, SAMPLES.stalled, SAMPLES.seen]) {
		const list = six.map((x, i) =>
			i === 5 ? { ...x, tier: s.tier, state: s.state, needs: s.needs } : x,
		);
		expect(ids(layout(list).hidden)).toEqual(["working4"]);
	}
	// The swap back: once it leaves Needs you (here, Ready), they swap back.
	const back = six.map((s, i) =>
		i === 5 ? { ...s, tier: "ready" as const, state: "ready" as const } : s,
	);
	expect(ids(layout(back).hidden)).toEqual(["working5"]);
});

test("a calm session in a slot that changes tier keeps its slot", () => {
	const six = many(6);
	const list = six.map((s, i) =>
		i === 0 ? { ...s, tier: "resting" as const, state: "idle" as const } : s,
	);
	expect(ids(layout(list).hidden)).toEqual(["working5"]);
});

test("more than 5 Needs-you: the first 5 in the core's order get a slot", () => {
	const knock = many(7, SAMPLES.permission);
	const { visible, hidden } = layout([SAMPLES.working, ...knock]);
	expect(ids(visible)).toEqual(ids(knock.slice(0, 5)));
	expect(ids(hidden)).toEqual(["working", "permission5", "permission6"]);
	expect(denCounts(hidden)).toEqual([
		{ tier: "needs_you", count: 2 },
		{ tier: "working", count: 1 },
	]);
	expect(denName(hidden)).toBe("3 more sessions: 2 need you, 1 working");
	expect(denName(hidden.slice(1, 2))).toBe("1 more session: 1 needs you");
});

test("den counts: one per non-zero tier, most urgent first", () => {
	const hidden = [
		SAMPLES.idle,
		SAMPLES.working,
		SAMPLES.ended,
		SAMPLES.ready,
		SAMPLES.stalled,
		SAMPLES.idle,
	];
	expect(denCounts(hidden)).toEqual([
		{ tier: "needs_you", count: 1 },
		{ tier: "ready", count: 1 },
		{ tier: "working", count: 1 },
		{ tier: "resting", count: 3 },
	]);
	expect(denName(hidden)).toBe(
		"6 more sessions: 1 needs you, 1 ready, 1 working, 3 resting",
	);
	expect(denCounts([])).toEqual([]);
	// Each tier has its own glyph, so a count never relies on color (§6).
	expect(new Set(TIERS.map(tierGlyph)).size).toBe(4);
});

test("eight-den: 3 Needs you and 2 Ready get slots; the den holds 1 ready, 2 working", () => {
	const { visible, hidden } = layout(EIGHT_DEN);
	expect(visible.map((s) => s.project)).toEqual([
		"web-shop",
		"api-gateway",
		"data-jobs",
		"mobile-app",
		"billing",
	]);
	expect(hidden.map((s) => s.project)).toEqual([
		"docs-site",
		"infra",
		"search",
	]);
	expect(denName(hidden)).toBe("3 more sessions: 1 ready, 2 working");
});

// WCAG 2.x relative luminance and contrast ratio (§10.5).
function luminance(hex: string): number {
	const [r, g, b] = [1, 3, 5].map((i) => {
		const c = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
		return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	}) as [number, number, number];
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
const contrast = (a: string, b: string) => {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [
		number,
		number,
	];
	return (hi + 0.05) / (lo + 0.05);
};

test("text on every pill meets WCAG AA; pills stand out on light and dark taskbars", () => {
	const LIGHT_TASKBAR = "#f3f3f3";
	const DARK_TASKBAR = "#202020";
	for (const fill of [...Object.values(TIER_FILL), DEN_FILL]) {
		expect(contrast(INK, fill)).toBeGreaterThanOrEqual(4.5);
		// Non-text contrast (3:1): the dark outline on light, the fill on dark.
		expect(contrast(fill, DARK_TASKBAR)).toBeGreaterThanOrEqual(3);
	}
	expect(contrast(INK, LIGHT_TASKBAR)).toBeGreaterThanOrEqual(3);
	// Glyph and dismiss marks are white on INK or RED.
	expect(contrast("#ffffff", INK)).toBeGreaterThanOrEqual(4.5);
	expect(contrast("#ffffff", RED)).toBeGreaterThanOrEqual(4.5);
});
