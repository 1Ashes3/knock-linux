// Sprite-sheet characters (ADR 0038): their clip timings and which cell shows when.
import { describe, expect, it } from "vitest";
import { CLIPS, type SheetJson, sheetCell, sheetClips } from "./art";

const sheet: SheetJson = {
	image: "s.png",
	frameWidth: 128,
	frameHeight: 128,
	clips: {
		sleep: { row: 0, frames: 4, fps: 4, loop: true },
		work: { row: 1, frames: 6, fps: 10, loop: true },
		wait: { row: 2, frames: 6, fps: 6, loop: true },
		knock: { row: 3, frames: 6, fps: 12, loop: true },
		rest: { row: 4, frames: 4, fps: 5, loop: true },
		present: { row: 5, frames: 3, fps: 10, loop: false },
	},
};

describe("sheet clips", () => {
	const clips = sheetClips(sheet);

	it("last their frames at their fps", () => {
		expect(clips.knock.durationMs).toBe(500);
		expect(clips.sleep.durationMs).toBe(1000);
		expect(clips.present).toMatchObject({ durationMs: 300, loop: false });
	});

	it("missing clips take their fallback's timing, or Knock's own for steps", () => {
		expect(clips.alert.durationMs).toBe(clips.knock.durationMs);
		expect(clips.pace.durationMs).toBe(clips.wait.durationMs);
		expect(clips.step_up.durationMs).toBe(CLIPS.step_up.durationMs);
		expect(clips.step_down.loop).toBe(false);
	});
});

describe("sheet cells", () => {
	it("the still pose is the first frame of wait", () => {
		expect(sheetCell(sheet, null, 1234)).toEqual({ row: 2, col: 0 });
		expect(sheetCell(sheet, "step_up", 200)).toEqual({ row: 2, col: 0 });
	});

	it("a looping row advances at its fps and wraps", () => {
		expect(sheetCell(sheet, "knock", 0)).toEqual({ row: 3, col: 0 });
		expect(sheetCell(sheet, "knock", 83)).toEqual({ row: 3, col: 0 });
		expect(sheetCell(sheet, "knock", 84)).toEqual({ row: 3, col: 1 });
		expect(sheetCell(sheet, "knock", 499)).toEqual({ row: 3, col: 5 });
		expect(sheetCell(sheet, "knock", 500)).toEqual({ row: 3, col: 0 });
	});

	it("a play-once row holds its last frame", () => {
		expect(sheetCell(sheet, "present", 250)).toEqual({ row: 5, col: 2 });
		expect(sheetCell(sheet, "present", 9999)).toEqual({ row: 5, col: 2 });
	});

	it("fallbacks draw the clip they stand in for", () => {
		expect(sheetCell(sheet, "alert", 0).row).toBe(3);
		expect(sheetCell(sheet, "pace", 0).row).toBe(2);
		const noPresent = {
			...sheet,
			clips: { ...sheet.clips, present: undefined },
		};
		expect(sheetCell(noPresent, "present", 0).row).toBe(4);
	});
});
