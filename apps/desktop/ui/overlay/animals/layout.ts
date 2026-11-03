// Where everything in animals mode goes (SPEC.md §9.2, §9.3, §10.1, §10.3). Pure: sizes
// in logical px, and text widths come from a `measure` function.
//
// The window is 6 slots wide (the core's cap, §9.2) and grows up from the taskbar:
//
//   chip area     chips in rows, each with a tail and a leader down to its own
//                 character; the hover tooltip is drawn here too
//   headroom      room for step_up and the sleep "z"
//   slots         the characters, first session nearest the clock, then "+N"
//
// Only characters and buttons are hit regions; everything else is click-through.
import type { HitRegion, SessionView, Size } from "../bridge";
import { SLOT_PX, stateLabel } from "../pill";
import { LIFT_UNITS } from "./controller";

/** §9.2: 6 slots at most (5 characters plus "+N"). */
export const MAX_SLOTS = 6;

export interface Metrics {
	/** Slot size: the strip height in plain mode. */
	s: number;
	/** Room above the slots for step_up and the "z": 16 viewBox units. */
	headroom: number;
	/** px per viewBox unit. */
	unit: number;
	font: number;
	lineHeight: number;
}

export function metrics(size: Size): Metrics {
	const s = SLOT_PX[size];
	const font = { small: 11, medium: 12, large: 13 }[size];
	return {
		s,
		headroom: s / 4,
		unit: s / 64,
		font,
		lineHeight: Math.round(font * 1.35),
	};
}

/** Chip and tooltip box details, in px. */
export const CHIP = {
	padX: 6,
	padY: 2,
	border: 1.5,
	/** Height of the tail under each chip; also the gap between rows. */
	tail: 6,
	rowGap: 2,
	/** Space kept between a chip and a neighbour's leader or chip. */
	gap: 6,
	minWidth: 30,
	maxLines: 3,
} as const;

export const TOOLTIP = { padX: 8, padY: 5, border: 1, lines: 4 } as const;

export type Measure = (text: string) => number;

/** Left edge of slot `i`; slot 0 is nearest the clock (the right edge). */
export function slotX(i: number, m: Metrics): number {
	return (MAX_SLOTS - 1 - i) * m.s;
}

export function stripWidth(slots: number, m: Metrics): number {
	return slots > 0 ? MAX_SLOTS * m.s : 0;
}

/** The space the tooltip always has: it never makes the window jump on hover. */
export function tooltipReserve(m: Metrics): number {
	return TOOLTIP.lines * m.lineHeight + 2 * (TOOLTIP.padY + TOOLTIP.border);
}

/** Top of the character box (slot), from the top of the strip. */
export const slotTop = (chipArea: number, m: Metrics) => chipArea + m.headroom;

/** A character's rectangle, raised by `step_up` while it knocks. */
export function characterRect(
	i: number,
	raised: boolean,
	chipArea: number,
	m: Metrics,
): HitRegion {
	return {
		x: slotX(i, m),
		y: slotTop(chipArea, m) - (raised ? LIFT_UNITS * m.unit : 0),
		width: m.s,
		height: m.s,
	};
}

/** The dismiss button on Failed and ended characters: the slot's top-right corner. */
export function dismissRect(
	i: number,
	chipArea: number,
	m: Metrics,
): HitRegion {
	const d = Math.round(m.s * 0.3);
	return {
		x: slotX(i, m) + m.s - d,
		y: slotTop(chipArea, m),
		width: d,
		height: d,
	};
}

/** What one character contributes to the hit regions. */
export interface HitInput {
	slot: number;
	raised: boolean;
	dismiss: boolean;
}

/** §8.5 `set_hit_regions`: character rectangles plus dismiss buttons, relative to the strip. */
export function hitRegions(
	chars: HitInput[],
	chipArea: number,
	m: Metrics,
): HitRegion[] {
	const out: HitRegion[] = [];
	for (const c of chars) {
		out.push(characterRect(c.slot, c.raised, chipArea, m));
		if (c.dismiss) out.push(dismissRect(c.slot, chipArea, m));
	}
	return out;
}

/** §10.1 chip text; null for states without one (working, idle, unused states). */
export function chipText(
	s: SessionView,
	now: number,
	stallMinutes: number,
): string | null {
	switch (s.state) {
		case "needs_input":
		case "failed":
		case "stalled":
		case "ready":
		case "ended_unexpectedly":
			return stateLabel(s, now, stallMinutes);
		default:
			return null;
	}
}

/**
 * Breaks `text` into at most `maxLines` lines no wider than `max`. Words longer than a
 * line (a long tool name) break anywhere. Text that still does not fit ends in "…".
 */
export function wrapText(
	text: string,
	max: number,
	measure: Measure,
	maxLines: number,
): string[] {
	const lines: string[] = [];
	let line = "";
	for (const word of text.split(/\s+/).filter(Boolean)) {
		const next = line ? `${line} ${word}` : word;
		if (measure(next) <= max) {
			line = next;
			continue;
		}
		if (line) lines.push(line);
		let rest = word;
		while (rest.length > 1 && measure(rest) > max) {
			let n = rest.length - 1;
			while (n > 1 && measure(rest.slice(0, n)) > max) n--;
			lines.push(rest.slice(0, n));
			rest = rest.slice(n);
		}
		line = rest;
	}
	if (line) lines.push(line);
	if (lines.length <= maxLines) return lines;
	const kept = lines.slice(0, maxLines);
	let last = `${kept[maxLines - 1]}…`;
	while (last.length > 1 && measure(last) > max) last = `${last.slice(0, -2)}…`;
	kept[maxLines - 1] = last;
	return kept;
}

export interface ChipInput {
	id: string;
	/** x of the leader: the centre of the character's slot. */
	anchor: number;
	text: string;
}

export interface ChipBox {
	id: string;
	anchor: number;
	lines: string[];
	x: number;
	/** Top, from the top of the chip area. */
	y: number;
	width: number;
	height: number;
	row: number;
}

const overlaps = (a: { x: number; width: number }, b: ChipBox) =>
	a.x < b.x + b.width + CHIP.gap && b.x < a.x + a.width + CHIP.gap;

/**
 * Lays out chips so that none overlaps another or covers a character, and every chip's
 * leader reaches its own character without crossing another chip.
 *
 * Chips are placed left to right. A chip may reach left over anything, but never past
 * the leader of the next chip to its right. Each goes in the lowest row where it overlaps
 * no chip and covers no leader coming down from a higher row. So rows grow up only when
 * chips would collide.
 */
export function layoutChips(
	inputs: ChipInput[],
	width: number,
	measure: Measure,
	m: Metrics,
): { chips: ChipBox[]; height: number } {
	const sorted = [...inputs].sort((a, b) => a.anchor - b.anchor);
	const inner = 2 * (CHIP.padX + CHIP.border);
	const placed: ChipBox[] = [];
	sorted.forEach((c, i) => {
		const next = sorted[i + 1];
		const right = next ? next.anchor - CHIP.gap : width;
		const lines = wrapText(c.text, right - inner, measure, CHIP.maxLines);
		const w = Math.min(
			right,
			Math.max(
				CHIP.minWidth,
				Math.ceil(Math.max(...lines.map(measure)) + inner),
			),
		);
		const x = Math.max(0, Math.min(c.anchor - w / 2, right - w));
		const height = lines.length * m.lineHeight + 2 * (CHIP.padY + CHIP.border);
		let row = 0;
		for (; ; row++) {
			const clash = placed.some(
				(p) =>
					(p.row === row && overlaps({ x, width: w }, p)) ||
					(p.row > row &&
						p.anchor > x - CHIP.gap &&
						p.anchor < x + w + CHIP.gap),
			);
			if (!clash) break;
		}
		placed.push({
			id: c.id,
			anchor: c.anchor,
			lines,
			x,
			y: 0,
			width: w,
			height,
			row,
		});
	});
	// Rows from the bottom up; each row as tall as its tallest chip, plus the tail below.
	const rows = placed.length ? Math.max(...placed.map((p) => p.row)) + 1 : 0;
	const rowHeight = Array.from({ length: rows }, (_, r) =>
		Math.max(...placed.filter((p) => p.row === r).map((p) => p.height)),
	);
	const height = rowHeight.reduce(
		(sum, h) => sum + h + CHIP.tail + CHIP.rowGap,
		0,
	);
	let bottom = height;
	const rowBottom = rowHeight.map((h) => {
		const b = bottom - CHIP.tail;
		bottom -= h + CHIP.tail + CHIP.rowGap;
		return b;
	});
	for (const p of placed) p.y = (rowBottom[p.row] as number) - p.height;
	return { chips: placed, height };
}

/** "For 3 min": how long the session has been in its state (FR-7). */
export function timeInState(since: string, now: number): string {
	const ms = now - Date.parse(since);
	if (!Number.isFinite(ms) || ms < 60_000) return "For under a minute";
	const min = Math.floor(ms / 60_000);
	if (min < 60) return `For ${min} min`;
	const h = Math.floor(min / 60);
	return min % 60 ? `For ${h} h ${min % 60} min` : `For ${h} h`;
}

/** The decision a Needs-you session is waiting on (FR-7); null otherwise. */
export function decisionType(s: SessionView): string | null {
	if (s.state !== "needs_input" || !s.needs) return null;
	if (s.needs.kind === "answer") return "Decision: answer a question";
	return s.needs.label
		? `Decision: permission to use ${s.needs.label}`
		: "Decision: a permission";
}

/**
 * FR-7, §10.3: project, state label, time in state and, for Needs you, the decision
 * type. Never anything else, and never agent text: everything here comes from fields the
 * core sends.
 */
export function tooltipLines(
	s: SessionView,
	now: number,
	stallMinutes: number,
): string[] {
	const lines = [
		s.project,
		stateLabel(s, now, stallMinutes),
		timeInState(s.since, now),
	];
	const decision = decisionType(s);
	if (decision) lines.push(decision);
	return lines;
}

/**
 * The tooltip box: centred over the character, kept inside the strip, ending at `bottom`
 * (just above the character). `widths` are the measured line widths.
 */
export function tooltipBox(
	widths: number[],
	anchor: number,
	bottom: number,
	width: number,
	m: Metrics,
): HitRegion {
	const inner = 2 * (TOOLTIP.padX + TOOLTIP.border);
	const w = Math.min(width, Math.ceil(Math.max(...widths) + inner + 1));
	const height =
		widths.length * m.lineHeight + 2 * (TOOLTIP.padY + TOOLTIP.border);
	const x = Math.max(0, Math.min(anchor - w / 2, width - w));
	return { x, y: bottom - height, width: w, height };
}
