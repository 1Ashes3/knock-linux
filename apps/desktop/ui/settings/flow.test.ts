import { expect, test } from "vitest";
import type { Monitor, PreviewOk } from "./core";
import {
	type Action,
	canGoNext,
	type Flow,
	flowReducer,
	lineDiff,
	MINUTES_ERROR,
	monitorLabel,
	monitorValue,
	parseMinutes,
	parseTime,
	quietHoursPatch,
	selectedMonitor,
	startFlow,
	startRepair,
	stepOf,
	TIME_ERROR,
} from "./flow";

const PREVIEW: PreviewOk = { ok: true, path: "p", before: "{}", after: "{ }" };
const run = (f: Flow, ...actions: Action[]) => actions.reduce(flowReducer, f);
const next: Action = { type: "next" };
const back: Action = { type: "back" };

test("screens in §10.4 order; the taskbar step only with more than one display", () => {
	expect(startFlow(2).steps).toEqual([
		"welcome",
		"taskbar",
		"connect",
		"signin",
		"test",
		"done",
	]);
	for (const n of [0, 1])
		expect(startFlow(n).steps).toEqual([
			"welcome",
			"connect",
			"signin",
			"test",
			"done",
		]);
	expect(stepOf(run(startFlow(1), next))).toBe("connect");
	expect(stepOf(run(startFlow(2), next))).toBe("taskbar");
});

test("Connect: nothing can be skipped until the hooks are written", () => {
	let f = run(startFlow(1), next);
	expect(f.connect).toEqual({ kind: "loading" });
	expect(canGoNext(f)).toBe(false);
	expect(run(f, next)).toBe(f);
	f = run(f, { type: "previewed", result: PREVIEW });
	expect(f.connect).toEqual({
		kind: "preview",
		preview: PREVIEW,
		applying: false,
	});
	expect(stepOf(run(f, next))).toBe("connect");
	f = run(f, { type: "apply" });
	expect(f.connect).toMatchObject({ kind: "preview", applying: true });
	// A second Confirm while writing does nothing.
	expect(run(f, { type: "apply" })).toBe(f);
	f = run(f, { type: "applied", result: { ok: true, backup: "b.json" } });
	expect(f.connect).toEqual({ kind: "connected", backup: "b.json" });
	expect(stepOf(run(f, next))).toBe("signin");
});

test("an invalid settings file stops at a message; Back, then Next reads it again", () => {
	let f = run(startFlow(1), next, {
		type: "previewed",
		result: { ok: false, message: "Not valid JSON." },
	});
	expect(f.connect).toEqual({
		kind: "failed",
		stage: "preview",
		message: "Not valid JSON.",
	});
	expect(canGoNext(f)).toBe(false);
	// Confirm can't be reached from here.
	expect(run(f, { type: "apply" })).toBe(f);
	f = run(f, back);
	expect(stepOf(f)).toBe("welcome");
	f = run(f, next);
	expect(f.connect).toEqual({ kind: "loading" });
});

test("a failed write shows the core's message and stays on Connect", () => {
	const f = run(
		startFlow(1),
		next,
		{ type: "previewed", result: PREVIEW },
		{ type: "apply" },
		{ type: "applied", result: { ok: false, message: "File in use." } },
	);
	expect(f.connect).toEqual({
		kind: "failed",
		stage: "apply",
		message: "File in use.",
	});
	expect(stepOf(run(f, next))).toBe("connect");
});

test("a late preview reply after leaving Connect is ignored", () => {
	const f = run(startFlow(1), next, back);
	expect(run(f, { type: "previewed", result: PREVIEW })).toBe(f);
	// Nor does a write result arrive without a Confirm.
	const g = run(startFlow(1), next, { type: "previewed", result: PREVIEW });
	expect(run(g, { type: "applied", result: { ok: true, backup: "b" } })).toBe(
		g,
	);
});

test("Back works from every screen and keeps a finished connection", () => {
	let f = run(
		startFlow(2),
		next,
		next,
		{ type: "previewed", result: PREVIEW },
		{ type: "apply" },
		{ type: "applied", result: { ok: true, backup: "b" } },
		next,
		next,
		{ type: "test", result: "played" },
		next,
	);
	expect(stepOf(f)).toBe("done");
	expect(canGoNext(f)).toBe(false);
	expect(run(f, next)).toBe(f);
	const seen = [stepOf(f)];
	for (let i = 0; i < 5; i++) {
		f = run(f, back);
		seen.push(stepOf(f));
	}
	expect(seen).toEqual([
		"done",
		"test",
		"signin",
		"connect",
		"taskbar",
		"welcome",
	]);
	expect(run(f, back)).toBe(f);
	expect(f.connect).toEqual({ kind: "connected", backup: "b" });
	// Coming back to the test screen asks again.
	expect(run(f, next, next, next, next).test).toBe("ready");
});

test("Repair: a new connection, then Connect, then Fixed after Confirm (two clicks)", () => {
	let f = startRepair();
	expect(f.steps).toEqual(["repair", "connect", "fixed"]);
	expect(f.repair).toEqual({ kind: "idle" });
	expect(canGoNext(f)).toBe(false);
	expect(run(f, next)).toBe(f);
	f = run(f, { type: "repair" });
	expect(f.repair).toEqual({ kind: "working" });
	// A second press while working does nothing.
	expect(run(f, { type: "repair" })).toBe(f);
	f = run(f, { type: "repaired", result: { ok: true } });
	expect(stepOf(f)).toBe("connect");
	expect(f.connect).toEqual({ kind: "loading" });
	expect(canGoNext(f)).toBe(false);
	f = run(
		f,
		{ type: "previewed", result: PREVIEW },
		{ type: "apply" },
		{ type: "applied", result: { ok: true, backup: "b" } },
	);
	expect(stepOf(f)).toBe("fixed");
	expect(f.connect).toEqual({ kind: "connected", backup: "b" });
	expect(canGoNext(f)).toBe(false);
	// Back keeps the work done: Connect stays connected, Repair stays repaired.
	f = run(f, back);
	expect(stepOf(f)).toBe("connect");
	expect(f.connect.kind).toBe("connected");
	f = run(f, back);
	expect(stepOf(f)).toBe("repair");
	expect(f.repair).toEqual({ kind: "repaired" });
	expect(run(f, { type: "repair" })).toBe(f);
	expect(stepOf(run(f, next))).toBe("connect");
});

test("Repair: a failure keeps the core's message and allows Try again", () => {
	let f = run(
		startRepair(),
		{ type: "repair" },
		{ type: "repaired", result: { ok: false, message: "No free port." } },
	);
	expect(stepOf(f)).toBe("repair");
	expect(f.repair).toEqual({ kind: "failed", message: "No free port." });
	expect(canGoNext(f)).toBe(false);
	f = run(f, { type: "repair" });
	expect(f.repair).toEqual({ kind: "working" });
	// A reply without a press is ignored.
	const idle = startRepair();
	expect(run(idle, { type: "repaired", result: { ok: true } })).toBe(idle);
});

test("Repair: a failed write stays on Connect, Back, then Next tries again", () => {
	let f = run(
		startRepair(),
		{ type: "repair" },
		{ type: "repaired", result: { ok: true } },
		{ type: "previewed", result: PREVIEW },
		{ type: "apply" },
		{ type: "applied", result: { ok: false, message: "In use." } },
	);
	expect(stepOf(f)).toBe("connect");
	expect(f.connect).toMatchObject({ kind: "failed", stage: "apply" });
	f = run(f, back, next);
	expect(stepOf(f)).toBe("connect");
	expect(f.connect).toEqual({ kind: "loading" });
});

test("setup itself never goes past Connected on its own", () => {
	const f = run(
		startFlow(1),
		next,
		{ type: "previewed", result: PREVIEW },
		{ type: "apply" },
		{ type: "applied", result: { ok: true, backup: "b" } },
	);
	expect(stepOf(f)).toBe("connect");
	// And Repair can't be pressed from setup.
	expect(run(f, { type: "repair" })).toBe(f);
});

test("minutes: whole numbers 1 to 1440", () => {
	for (const ok of ["1", "10", " 1440 "])
		expect(parseMinutes(ok)).toEqual({ value: Number(ok) });
	for (const bad of ["", "0", "1441", "1.5", "-3", "ten", "1e2"])
		expect(parseMinutes(bad)).toEqual({ error: MINUTES_ERROR });
});

test("quiet-hours times: HH:MM, 00:00 to 23:59", () => {
	for (const ok of ["00:00", "08:00", "22:30", "23:59"])
		expect(parseTime(ok)).toEqual({ value: ok });
	for (const bad of ["", "8:00", "24:00", "12:60", "10:00 PM", "22:00:00"])
		expect(parseTime(bad)).toEqual({ error: TIME_ERROR });
});

test("quiet-hours patches carry the whole object", () => {
	const qh = { enabled: false, start: "22:00", end: "08:00" };
	expect(quietHoursPatch(qh, { start: "21:00" })).toEqual({
		quietHours: { enabled: false, start: "21:00", end: "08:00" },
	});
	expect(quietHoursPatch(qh, { enabled: true })).toEqual({
		quietHours: { enabled: true, start: "22:00", end: "08:00" },
	});
});

test("taskbar choice: the main display is stored as primary", () => {
	const ms: Monitor[] = [
		{ id: "A", name: "DELL", primary: true, width: 2560, height: 1440 },
		{ id: "B", name: " ", primary: false, width: 1920, height: 1080 },
	];
	expect(ms.map(monitorValue)).toEqual(["primary", "B"]);
	expect(selectedMonitor("primary", ms)).toBe(ms[0]);
	expect(selectedMonitor("A", ms)).toBe(ms[0]);
	expect(selectedMonitor("B", ms)).toBe(ms[1]);
	expect(selectedMonitor("gone", ms)).toBeUndefined();
	expect(ms.map(monitorLabel)).toEqual([
		"DELL (main display), 2560 × 1440",
		"Display 2, 1920 × 1080",
	]);
});

test("taskbar labels: repeated names always show the resolution; exact twins are numbered", () => {
	const g = "Generic PnP Monitor";
	const ms: Monitor[] = [
		{ id: "A", name: g, primary: true, width: 1920, height: 1080 },
		{ id: "B", name: g, primary: false, width: 1920, height: 1080 },
		{ id: "C", name: g, primary: false, width: 2560, height: 1440 },
		{ id: "D", name: "", primary: false, width: 1920, height: 1080 },
		{ id: "E", name: " ", primary: false, width: 1920, height: 1080 },
	];
	const labels = ms.map(monitorLabel);
	expect(labels).toEqual([
		"Display 1: Generic PnP Monitor (main display), 1920 × 1080",
		"Display 2: Generic PnP Monitor, 1920 × 1080",
		"Generic PnP Monitor, 2560 × 1440",
		"Display 4, 1920 × 1080",
		"Display 5, 1920 × 1080",
	]);
	expect(new Set(labels).size).toBe(labels.length);
});

test("line diff: exact, with every line of both files accounted for", () => {
	const before = '{\n  "model": "opus"\n}\n';
	const after = '{\n  "model": "opus",\n  "hooks": {}\n}\n';
	expect(lineDiff(before, after)).toEqual([
		{ kind: "same", text: "{" },
		{ kind: "del", text: '  "model": "opus"' },
		{ kind: "add", text: '  "model": "opus",' },
		{ kind: "add", text: '  "hooks": {}' },
		{ kind: "same", text: "}" },
		{ kind: "same", text: "" },
	]);
	expect(lineDiff("", "a")).toEqual([{ kind: "add", text: "a" }]);
	expect(lineDiff("a\r\nb", "a\nb").every((l) => l.kind === "same")).toBe(true);
	// Interleaved changes: rebuild both sides from the diff.
	const a = ["k", "x", "m", "y", "n"].join("\n");
	const b = ["k", "m", "z", "n", "w"].join("\n");
	const d = lineDiff(a, b);
	const side = (skip: string) =>
		d
			.filter((l) => l.kind !== skip)
			.map((l) => l.text)
			.join("\n");
	expect(side("add")).toBe(a);
	expect(side("del")).toBe(b);
	expect(d.filter((l) => l.kind === "same")).toHaveLength(3);
});
