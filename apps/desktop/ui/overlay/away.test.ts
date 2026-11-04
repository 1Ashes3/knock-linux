import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
	AWAY,
	AWAY_SHOW_MS,
	awayContent,
	awayFor,
	awayHeight,
	awayMetrics,
	awayName,
	cardOffset,
	createAwayCard,
	MAX_WIDTH_SLOTS,
	OUTCOME_LABEL,
	withCard,
} from "./away";
import type { AwaySummary } from "./bridge";
import { awaySample } from "./dev/samples";
import { createStripStore } from "./store";

const NOW = Date.parse("2026-11-01T12:00:00Z");
const summary = (n: number, minutesAway = 25): AwaySummary =>
	awaySample(n, NOW - (minutesAway - 25) * 60_000);

test("each item is its project and its outcome in plain words, nothing else", () => {
	const c = awayContent(summary(3), NOW);
	expect(c.heading).toBe("While you were away (25 min)");
	expect(c.rows).toEqual([
		{
			id: "s21",
			project: "web-shop",
			outcome: "ready",
			label: "Ready to review",
		},
		{
			id: "s23",
			project: "data-jobs",
			outcome: "needs_you",
			label: "Needs you",
		},
		{
			id: "s24",
			project: "mobile-app",
			outcome: "failed",
			label: "Stopped with an error",
		},
	]);
	expect(c.more).toBe(0);
	expect(OUTCOME_LABEL).toEqual({
		ready: "Ready to review",
		failed: "Stopped with an error",
		needs_you: "Needs you",
	});
});

test("at most 5 items, then 'and N more'", () => {
	const five = awayContent(summary(5), NOW);
	expect(five.rows).toHaveLength(5);
	expect(five.more).toBe(0);
	const seven = awayContent(summary(7), NOW);
	expect(seven.rows.map((r) => r.project)).toEqual([
		"web-shop",
		"data-jobs",
		"mobile-app",
		"docs-site",
		"billing",
	]);
	expect(seven.more).toBe(2);
	expect(awayName(seven)).toBe(
		"While you were away (25 min): web-shop, Ready to review; data-jobs, Needs you; " +
			"mobile-app, Stopped with an error; docs-site, Ready to review; billing, Needs you; " +
			"and 2 more. Click to close.",
	);
	expect(awayName(awayContent(summary(1), NOW))).toBe(
		"While you were away (25 min): web-shop, Ready to review. Click to close.",
	);
});

test("how long you were away, from `since` to when the card arrived", () => {
	const at = (min: number) =>
		awayFor(new Date(NOW - min * 60_000).toISOString(), NOW);
	expect(at(10)).toBe("10 min");
	expect(at(59.9)).toBe("59 min");
	expect(at(60)).toBe("1 h");
	expect(at(125)).toBe("2 h 5 min");
	expect(at(0.2)).toBe("1 min");
	expect(awayFor("not a time", NOW)).toBeNull();
	expect(awayFor(new Date(NOW + 60_000).toISOString(), NOW)).toBeNull();
	expect(awayContent({ since: "?", items: [] }, NOW).heading).toBe(
		"While you were away",
	);
});

test("card height follows its rows; the window grows up to fit it, never past 6 slots", () => {
	const m = awayMetrics("medium");
	const one = awayHeight(awayContent(summary(1), NOW), m);
	const five = awayHeight(awayContent(summary(5), NOW), m);
	const seven = awayHeight(awayContent(summary(7), NOW), m);
	expect(five - one).toBe(4 * (m.row + AWAY.rowGap));
	expect(seven - five).toBe(AWAY.rowGap + m.lineHeight);
	expect(awayMetrics("small").width).toBeLessThan(m.width);
	expect(awayMetrics("large").width).toBeGreaterThan(m.width);
	const card = { width: 264, height: 120 };
	expect(withCard({ width: 128, height: 64 }, null, 64)).toEqual({
		width: 128,
		height: 64,
	});
	// Plain, 2 pills: as wide as the card, the card stacked on top.
	expect(withCard({ width: 128, height: 64 }, card, 64)).toEqual({
		width: 264,
		height: 64 + AWAY.gap + 120,
	});
	expect(cardOffset({ width: 128, height: 64 }, card)).toEqual({
		x: 128 - 264,
		y: -(AWAY.gap + 120),
	});
	// Animals: the window stays 6 slots wide and grows up.
	expect(withCard({ width: 384, height: 156 }, card, 64)).toEqual({
		width: 384,
		height: 156 + AWAY.gap + 120,
	});
	expect(cardOffset({ width: 384, height: 156 }, card)).toEqual({
		x: 120,
		y: -(AWAY.gap + 120),
	});
	// No strip: just the card, on the taskbar, with no gap.
	expect(withCard({ width: 0, height: 64 }, card, 64)).toEqual(card);
	expect(cardOffset({ width: 0, height: 64 }, card)).toEqual({
		x: -264,
		y: 64 - 120,
	});
	// §9.2: never wider than 6 slots, whatever is asked.
	expect(withCard({ width: 1000, height: 64 }, null, 48).width).toBe(288);
	expect(withCard({ width: 0, height: 0 }, card, 40).width).toBe(240);
	for (const size of ["small", "medium", "large"] as const) {
		const slot = { small: 48, medium: 64, large: 80 }[size];
		expect(awayMetrics(size).width).toBeLessThanOrEqual(MAX_WIDTH_SLOTS * slot);
	}
});

beforeEach(() => {
	vi.useFakeTimers();
	vi.setSystemTime(NOW);
});
afterEach(() => vi.useRealTimers());

function setup() {
	const store = createStripStore();
	const send = vi.fn();
	const card = createAwayCard(store, send);
	return { store, send, card };
}

test("folds away after 60 s and calls dismiss_away_summary once", () => {
	const { store, send } = setup();
	store.getState().applyAway(summary(2));
	expect(store.getState().away?.at).toBe(NOW);
	vi.advanceTimersByTime(AWAY_SHOW_MS - 1);
	expect(store.getState().away).not.toBeNull();
	expect(send).not.toHaveBeenCalled();
	vi.advanceTimersByTime(1);
	expect(store.getState().away).toBeNull();
	expect(send).toHaveBeenCalledTimes(1);
	expect(send).toHaveBeenCalledWith("dismiss_away_summary");
	vi.advanceTimersByTime(10 * AWAY_SHOW_MS);
	expect(send).toHaveBeenCalledTimes(1);
});

test("a click folds it away at once; the 60 s timer then does nothing", () => {
	const { store, send, card } = setup();
	store.getState().applyAway(summary(2));
	vi.advanceTimersByTime(5000);
	card.dismiss();
	expect(store.getState().away).toBeNull();
	expect(send).toHaveBeenCalledTimes(1);
	card.dismiss(); // a second click on a card that is gone
	vi.advanceTimersByTime(AWAY_SHOW_MS);
	expect(send).toHaveBeenCalledTimes(1);
	expect(vi.getTimerCount()).toBe(0);
});

test("a newer summary replaces the card and gets its own 60 s", () => {
	const { store, send } = setup();
	store.getState().applyAway(summary(2));
	vi.advanceTimersByTime(50_000);
	const newer = summary(4, 40);
	store.getState().applyAway(newer);
	expect(store.getState().away?.summary).toBe(newer);
	expect(send).not.toHaveBeenCalled(); // replacing is not dismissing
	vi.advanceTimersByTime(AWAY_SHOW_MS - 1);
	expect(store.getState().away?.summary).toBe(newer);
	vi.advanceTimersByTime(1);
	expect(store.getState().away).toBeNull();
	expect(send).toHaveBeenCalledTimes(1);
});

test("stop() leaves no timer behind", () => {
	const { store, send, card } = setup();
	store.getState().applyAway(summary(1));
	card.stop();
	vi.advanceTimersByTime(AWAY_SHOW_MS);
	expect(send).not.toHaveBeenCalled();
	expect(vi.getTimerCount()).toBe(0);
});
