// The "While you were away" card (SPEC.md FR-9, §10.3, ADR 0034). The core picks the
// sessions and their outcomes; the UI only words them and folds the card away. Shows
// nothing but project names and the three outcomes. Pure, apart from `createAwayCard`.
import { type AwayItem, type AwaySummary, call, type Size } from "./bridge";
import type { Glyph } from "./pill";
import type { StripStore } from "./store";

/** FR-9: the card folds away by itself after 60 s. */
export const AWAY_SHOW_MS = 60_000;

/** Lead decision: at most 5 items, then "and N more". */
export const AWAY_MAX_ITEMS = 5;

export const OUTCOME_LABEL: Record<AwayItem["outcome"], string> = {
	ready: "Ready to review",
	failed: "Stopped with an error",
	needs_you: "Needs you",
};

/** The glyph and tier color of each outcome, as on the pills. */
export const OUTCOME_GLYPH: Record<AwayItem["outcome"], Glyph> = {
	ready: "check",
	failed: "failed",
	needs_you: "ask",
};

/** "25 min", "1 h 5 min", "2 h"; null when `since` can't be read. */
export function awayFor(since: string, at: number): string | null {
	const ms = at - Date.parse(since);
	if (!Number.isFinite(ms) || ms < 0) return null;
	const min = Math.max(1, Math.floor(ms / 60_000));
	if (min < 60) return `${min} min`;
	const h = Math.floor(min / 60);
	return min % 60 ? `${h} h ${min % 60} min` : `${h} h`;
}

export interface AwayRow {
	id: string;
	project: string;
	outcome: AwayItem["outcome"];
	label: string;
}

export interface AwayContent {
	heading: string;
	rows: AwayRow[];
	/** Items beyond the first 5. */
	more: number;
}

/** What the card says. `at` is when the summary arrived, so the duration never ticks. */
export function awayContent(summary: AwaySummary, at: number): AwayContent {
	const time = awayFor(summary.since, at);
	return {
		heading: time ? `While you were away (${time})` : "While you were away",
		rows: summary.items.slice(0, AWAY_MAX_ITEMS).map((i) => ({
			id: i.id,
			project: i.project,
			outcome: i.outcome,
			label: OUTCOME_LABEL[i.outcome],
		})),
		more: Math.max(0, summary.items.length - AWAY_MAX_ITEMS),
	};
}

/** "and 2 more" */
export const moreText = (n: number) => `and ${n} more`;

/** §10.5: what Narrator reads, and announces when the card appears. */
export function awayName(content: AwayContent): string {
	const rows = content.rows.map((r) => `${r.project}, ${r.label}`);
	if (content.more) rows.push(moreText(content.more));
	return `${content.heading}: ${rows.join("; ")}. Click to close.`;
}

/** Card box details, in logical px. */
export const AWAY = {
	/** Between the card and the strip below it. */
	gap: 8,
	padX: 8,
	padY: 6,
	border: 1.5,
	/** Below the heading. */
	headGap: 4,
	rowGap: 2,
} as const;

export interface AwayMetrics {
	width: number;
	font: number;
	lineHeight: number;
	/** One item row: a line plus room for the outcome chip. */
	row: number;
}

export function awayMetrics(size: Size): AwayMetrics {
	const font = { small: 11, medium: 12, large: 13 }[size];
	const lineHeight = Math.round(font * 1.35);
	return {
		width: { small: 236, medium: 264, large: 292 }[size],
		font,
		lineHeight,
		row: lineHeight + 4,
	};
}

/** The card's height for this content. */
export function awayHeight(content: AwayContent, m: AwayMetrics): number {
	const rows = content.rows.length;
	const lines =
		m.lineHeight +
		(rows ? AWAY.headGap + rows * m.row + (rows - 1) * AWAY.rowGap : 0) +
		(content.more ? AWAY.rowGap + m.lineHeight : 0);
	return Math.ceil(lines + 2 * (AWAY.padY + AWAY.border));
}

/**
 * The window size with the card stacked above the strip, right edges level, so the card
 * never covers a character, chip, leader, den or button. The window grows up, never past
 * 6 slots wide (§9.2: the core caps it there). With no strip the card sits on the taskbar.
 * `strip` is the strip's own size; `slot` the slot size.
 */
export function withCard(
	strip: { width: number; height: number },
	card: { width: number; height: number } | null,
	slot: number,
): { width: number; height: number } {
	const max = MAX_WIDTH_SLOTS * slot;
	if (!card) return { width: Math.min(strip.width, max), height: strip.height };
	if (!strip.width)
		return { width: Math.min(card.width, max), height: card.height };
	return {
		width: Math.min(Math.max(strip.width, card.width), max),
		height: strip.height + AWAY.gap + card.height,
	};
}

/** §9.2: the window is at most 6 slots wide (5 characters plus the den). */
export const MAX_WIDTH_SLOTS = 6;

/**
 * Where the card goes, relative to the strip's top-left, for a strip `width` x `height`:
 * above it with a gap, right edges level; on the taskbar when there is no strip.
 */
export function cardOffset(
	strip: { width: number; height: number },
	card: { width: number; height: number },
): { x: number; y: number } {
	return {
		x: strip.width - card.width,
		y: strip.width ? -(AWAY.gap + card.height) : strip.height - card.height,
	};
}

/**
 * Folds the card away after 60 s, or when `dismiss` is called (a click). Either way it
 * calls `dismiss_away_summary()` once (ADR 0034). A new summary restarts the 60 s.
 */
export function createAwayCard(
	store: StripStore,
	send: (cmd: string) => unknown = call,
): { dismiss: () => void; stop: () => void } {
	let timer: ReturnType<typeof setTimeout> | undefined;
	const dismiss = () => {
		if (!store.getState().away) return;
		store.getState().clearAway();
		void send("dismiss_away_summary");
	};
	const stop = store.subscribe((s, prev) => {
		if (s.away === prev.away) return;
		clearTimeout(timer);
		timer = s.away ? setTimeout(dismiss, AWAY_SHOW_MS) : undefined;
	});
	return {
		dismiss,
		stop: () => {
			stop();
			clearTimeout(timer);
		},
	};
}
