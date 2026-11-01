import { expect, test } from "vitest";
import type { SessionView } from "./bridge";
import { SAMPLES } from "./dev/samples";
import {
	accessibleName,
	dismissible,
	glyph,
	INK,
	knocks,
	layout,
	OVERFLOW_FILL,
	overflowName,
	RED,
	shortWord,
	TIER_FILL,
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

test("above 5: 5 pills plus +N, urgent tiers keep their pill, core order kept", () => {
	const idle = many(3, SAMPLES.idle);
	const working = many(2);
	const urgent = [SAMPLES.permission, SAMPLES.ready];
	const sessions = [
		idle[0],
		working[0],
		urgent[0],
		idle[1],
		working[1],
		urgent[1],
		idle[2],
	] as SessionView[];
	const { visible, hidden } = layout(sessions);
	expect(visible).toEqual([
		idle[0],
		working[0],
		urgent[0],
		working[1],
		urgent[1],
	]);
	expect(hidden).toEqual([idle[1], idle[2]]);
	expect(overflowName(hidden)).toBe("2 more sessions");
});

test("+N names hidden Needs-you sessions", () => {
	const { hidden } = layout(many(7, SAMPLES.permission));
	expect(hidden).toHaveLength(2);
	expect(overflowName(hidden)).toBe("2 more sessions, 2 need you");
	expect(overflowName(hidden.slice(1))).toBe("1 more session, 1 needs you");
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
	for (const fill of [...Object.values(TIER_FILL), OVERFLOW_FILL]) {
		expect(contrast(INK, fill)).toBeGreaterThanOrEqual(4.5);
		// Non-text contrast (3:1): the dark outline on light, the fill on dark.
		expect(contrast(fill, DARK_TASKBAR)).toBeGreaterThanOrEqual(3);
	}
	expect(contrast(INK, LIGHT_TASKBAR)).toBeGreaterThanOrEqual(3);
	// Glyph and dismiss marks are white on INK or RED.
	expect(contrast("#ffffff", INK)).toBeGreaterThanOrEqual(4.5);
	expect(contrast("#ffffff", RED)).toBeGreaterThanOrEqual(4.5);
});
