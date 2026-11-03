import { expect, test } from "vitest";
import type { SessionView, State } from "../bridge";
import { SAMPLES } from "../dev/samples";
import {
	CHIP,
	type ChipBox,
	type ChipInput,
	characterRect,
	chipText,
	decisionType,
	dismissRect,
	hitRegions,
	layoutChips,
	MAX_SLOTS,
	metrics,
	slotX,
	stripWidth,
	TOOLTIP,
	timeInState,
	tooltipBox,
	tooltipLines,
	tooltipReserve,
	wrapText,
} from "./layout";

const NOW = Date.parse("2026-11-01T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const at3 = (s: SessionView): SessionView => ({ ...s, since: ago(3 * 60_000) });
/** A fake font: 7 px per character. */
const measure = (t: string) => t.length * 7;
const m = metrics("medium");

test("chip texts are exactly the §10.1 texts; working, idle and unused states have none", () => {
	const cases: [SessionView, string | null][] = [
		[SAMPLES.permission, "Needs permission: PowerShell"],
		[SAMPLES.permission_no_tool, "Needs permission"],
		[SAMPLES.question, "Has a question"],
		[SAMPLES.seen, "Waiting for you"],
		[SAMPLES.failed, "Stopped with an error"],
		[at3(SAMPLES.stalled), "No progress for 13 min"],
		[SAMPLES.ready, "Done, ready to review"],
		[SAMPLES.working, null],
		[SAMPLES.idle, null],
		[SAMPLES.ended, "Session ended"],
	];
	for (const [s, text] of cases) expect(chipText(s, NOW, 10)).toBe(text);
	for (const state of [
		"waiting_agent",
		"communicating",
		"blocked_fixable",
		"paused",
		"cancelled",
	] as State[])
		expect(chipText({ ...SAMPLES.working, state }, NOW, 10)).toBeNull();
});

test("tooltip: project, state label, time in state, and the decision type for Needs you only", () => {
	expect(tooltipLines(at3(SAMPLES.permission), NOW, 10)).toEqual([
		"knock",
		"Needs permission: PowerShell",
		"For 3 min",
		"Decision: permission to use PowerShell",
	]);
	expect(tooltipLines(at3(SAMPLES.permission_no_tool), NOW, 10)).toEqual([
		"api",
		"Needs permission",
		"For 3 min",
		"Decision: a permission",
	]);
	expect(tooltipLines(at3(SAMPLES.question), NOW, 10)).toEqual([
		"website",
		"Has a question",
		"For 3 min",
		"Decision: answer a question",
	]);
	expect(tooltipLines(at3(SAMPLES.seen), NOW, 10)).toEqual([
		"notes",
		"Waiting for you",
		"For 3 min",
		"Decision: permission to use Bash",
	]);
	expect(tooltipLines(at3(SAMPLES.failed), NOW, 10)).toEqual([
		"billing-service",
		"Stopped with an error",
		"For 3 min",
	]);
	expect(tooltipLines(at3(SAMPLES.working), NOW, 10)).toEqual([
		"mobile-app",
		"Working",
		"For 3 min",
	]);
	expect(tooltipLines(at3(SAMPLES.idle), NOW, 10)).toEqual([
		"scripts",
		"Idle",
		"For 3 min",
	]);
});

test("the tooltip never shows anything but those fields (FR-7)", () => {
	// Fields that are not FR-7 items stay out, whatever they hold.
	const s: SessionView = {
		...at3(SAMPLES.working),
		activity: "edit",
		precision: "exact_window",
		simulated: true,
		animal: "owl",
		id: "a-session-id",
	};
	const text = tooltipLines(s, NOW, 10).join("\n");
	for (const leak of ["edit", "exact_window", "owl", "a-session-id", "true"])
		expect(text).not.toContain(leak);
	expect(decisionType(SAMPLES.failed)).toBeNull();
	expect(decisionType(SAMPLES.ready)).toBeNull();
});

test("time in state", () => {
	expect(timeInState(ago(0), NOW)).toBe("For under a minute");
	expect(timeInState(ago(59_999), NOW)).toBe("For under a minute");
	expect(timeInState(ago(60_000), NOW)).toBe("For 1 min");
	expect(timeInState(ago(59 * 60_000), NOW)).toBe("For 59 min");
	expect(timeInState(ago(60 * 60_000), NOW)).toBe("For 1 h");
	expect(timeInState(ago(125 * 60_000), NOW)).toBe("For 2 h 5 min");
	expect(timeInState("garbage", NOW)).toBe("For under a minute");
	expect(timeInState(ago(-5000), NOW)).toBe("For under a minute");
});

test("6 slots wide whenever anything shows; slot 0 is nearest the clock", () => {
	for (const size of ["small", "medium", "large"] as const) {
		const mm = metrics(size);
		expect(stripWidth(1, mm)).toBe(MAX_SLOTS * mm.s);
		expect(stripWidth(0, mm)).toBe(0);
		expect(slotX(0, mm)).toBe(5 * mm.s);
		expect(slotX(5, mm)).toBe(0);
	}
	expect([48, 64, 80]).toEqual(
		(["small", "medium", "large"] as const).map((s) => metrics(s).s),
	);
});

test("hit regions: one rectangle per character, raised while it knocks, plus dismiss buttons", () => {
	const chipArea = 80;
	const top = chipArea + m.headroom; // 96
	expect(characterRect(0, false, chipArea, m)).toEqual({
		x: 320,
		y: top,
		width: 64,
		height: 64,
	});
	// step_up lifts 12 of 64 units: 12 px at medium.
	expect(characterRect(1, true, chipArea, m)).toEqual({
		x: 256,
		y: top - 12,
		width: 64,
		height: 64,
	});
	expect(dismissRect(2, chipArea, m)).toEqual({
		x: 192 + 64 - 19,
		y: top,
		width: 19,
		height: 19,
	});
	expect(
		hitRegions(
			[
				{ slot: 0, raised: true, dismiss: false },
				{ slot: 1, raised: false, dismiss: true },
			],
			chipArea,
			m,
		),
	).toEqual([
		{ x: 320, y: top - 12, width: 64, height: 64 },
		{ x: 256, y: top, width: 64, height: 64 },
		{ x: 256 + 64 - 19, y: top, width: 19, height: 19 },
	]);
	expect(hitRegions([], chipArea, m)).toEqual([]);
	// Small and large scale with the slot.
	const small = metrics("small");
	expect(characterRect(0, true, 0, small)).toEqual({
		x: 240,
		y: 12 - 9,
		width: 48,
		height: 48,
	});
});

test("wrapText keeps words whole, breaks over-long ones, and ends with … past maxLines", () => {
	expect(wrapText("Has a question", 200, measure, 3)).toEqual([
		"Has a question",
	]);
	expect(wrapText("Needs permission: PowerShell", 130, measure, 3)).toEqual([
		"Needs permission:",
		"PowerShell",
	]);
	expect(wrapText("Needs permission: PowerShell", 84, measure, 3)).toEqual([
		"Needs",
		"permission:",
		"PowerShell",
	]);
	// "mcp__github__merge" breaks inside the word, and the 4th line is cut with "…".
	expect(
		wrapText("Needs permission: mcp__github__merge", 84, measure, 3),
	).toEqual(["Needs", "permission:", "mcp__github…"]);
	for (const l of wrapText("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 70, measure, 2))
		expect(measure(l)).toBeLessThanOrEqual(70);
	// Narrower than one character: still terminates, one character per line.
	expect(wrapText("abc", 3, measure, 5)).toEqual(["a", "b", "c"]);
});

/** Checks every rule of the chip layout. */
function checkChips(
	inputs: ChipInput[],
	chips: ChipBox[],
	height: number,
	width: number,
) {
	expect(chips).toHaveLength(inputs.length);
	for (const c of chips) {
		// Inside the strip and above the characters (the chip area).
		expect(c.x).toBeGreaterThanOrEqual(0);
		expect(c.x + c.width).toBeLessThanOrEqual(width + 1e-9);
		expect(c.y).toBeGreaterThanOrEqual(0);
		expect(c.y + c.height + CHIP.tail).toBeLessThanOrEqual(height + 1e-9);
		// The tail sits under the chip.
		expect(c.anchor).toBeGreaterThan(c.x);
		expect(c.anchor).toBeLessThan(c.x + c.width);
		for (const o of chips) {
			if (o === c) continue;
			// Never overlapping another chip.
			const apart =
				c.x + c.width <= o.x ||
				o.x + o.width <= c.x ||
				c.y + c.height <= o.y ||
				o.y + o.height <= c.y;
			expect(apart, `${c.id} overlaps ${o.id}`).toBe(true);
			// A leader from a higher chip never crosses a lower one.
			if (o.y + o.height <= c.y)
				expect(
					o.anchor < c.x || o.anchor > c.x + c.width,
					`${o.id}'s leader crosses ${c.id}`,
				).toBe(true);
		}
	}
}

const chipFor = (slot: number, text: string): ChipInput => ({
	id: `s${slot}`,
	anchor: slotX(slot, m) + m.s / 2,
	text,
});

test("one chip sits in the lowest row, centred over its character", () => {
	const width = stripWidth(1, m);
	const { chips, height } = layoutChips(
		[chipFor(2, "Has a question")],
		width,
		measure,
		m,
	);
	expect(chips[0]?.row).toBe(0);
	const c = chips[0] as ChipBox;
	expect(c.x + c.width / 2).toBeCloseTo(c.anchor);
	expect(height).toBe(c.height + CHIP.tail + CHIP.rowGap);
	expect(c.lines).toEqual(["Has a question"]);
});

test("chips near the clock shift left to stay inside the strip", () => {
	const width = stripWidth(1, m);
	const { chips } = layoutChips(
		[chipFor(0, "Needs permission: PowerShell")],
		width,
		measure,
		m,
	);
	expect((chips[0] as ChipBox).x + (chips[0] as ChipBox).width).toBe(width);
});

test("neighbouring chips stack into rows; leaders never cross a chip", () => {
	const width = stripWidth(6, m);
	const inputs = [
		chipFor(0, "Needs permission: PowerShell"),
		chipFor(1, "Has a question"),
		chipFor(2, "Stopped with an error"),
		chipFor(3, "Done, ready to review"),
		chipFor(4, "No progress for 12 min"),
	];
	const { chips, height } = layoutChips(inputs, width, measure, m);
	checkChips(inputs, chips, height, width);
	expect(Math.max(...chips.map((c) => c.row))).toBeGreaterThan(0);
});

test("far-apart chips share a row", () => {
	const width = stripWidth(6, m);
	const inputs = [chipFor(0, "Session ended"), chipFor(4, "Waiting for you")];
	const { chips } = layoutChips(inputs, width, measure, m);
	expect(chips.map((c) => c.row)).toEqual([0, 0]);
});

test("chip layout rules hold for random strips at every size", () => {
	const texts = [
		"Needs permission: PowerShell",
		"Needs permission: mcp__a_very_long_server__and_tool_name",
		"Has a question",
		"Waiting for you",
		"Stopped with an error",
		"No progress for 125 min",
		"Done, ready to review",
		"Session ended",
	];
	let seed = 7;
	const rand = () => {
		seed = (seed * 16807) % 2147483647;
		return seed / 2147483647;
	};
	for (const size of ["small", "medium", "large"] as const) {
		const mm = metrics(size);
		for (let run = 0; run < 200; run++) {
			const inputs: ChipInput[] = [];
			for (let slot = 0; slot < 5; slot++)
				if (rand() < 0.7)
					inputs.push({
						id: `s${slot}`,
						anchor: slotX(slot, mm) + mm.s / 2,
						text: texts[Math.floor(rand() * texts.length)] as string,
					});
			const width = stripWidth(6, mm);
			const { chips, height } = layoutChips(inputs, width, measure, mm);
			checkChips(inputs, chips, height, width);
			for (const c of chips) expect(c.lines.length).toBeLessThanOrEqual(3);
		}
	}
});

test("the tooltip stays inside the strip and ends above its character", () => {
	const width = stripWidth(3, m);
	const widths = [40, 200, 60, 260];
	const anchor = slotX(0, m) + m.s / 2;
	const box = tooltipBox(widths, anchor, 120, width, m);
	expect(box.x + box.width).toBeLessThanOrEqual(width);
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.y + box.height).toBe(120);
	expect(box.height).toBeLessThanOrEqual(tooltipReserve(m));
	expect(box.width).toBe(
		Math.ceil(260 + 2 * (TOOLTIP.padX + TOOLTIP.border) + 1),
	);
	// Wider than the strip: clamped to it.
	expect(tooltipBox([5000], anchor, 120, width, m)).toMatchObject({
		x: 0,
		width,
	});
	// Even for a raised character, the reserve leaves room above it.
	const top = characterRect(0, true, tooltipReserve(m), m).y;
	expect(
		tooltipBox(widths, anchor, top - 3, width, m).y,
	).toBeGreaterThanOrEqual(0);
});
