// Pure logic for the setup flow (SPEC.md §10.4) and the Settings screen (§8.6).
// Nothing here talks to the core; the components do, and feed results in.
import type { Settings } from "../overlay/bridge";
import type { Failure, Monitor, Preview, PreviewOk, Written } from "./core";

export type Step =
	| "welcome"
	| "taskbar"
	| "connect"
	| "signin"
	| "test"
	| "done"
	| "repair"
	| "fixed";

const STEPS: Step[] = [
	"welcome",
	"taskbar",
	"connect",
	"signin",
	"test",
	"done",
];

/** §10.4: the taskbar step only when more than one display is connected. */
export function stepsFor(monitorCount: number): Step[] {
	return monitorCount > 1 ? STEPS : STEPS.filter((s) => s !== "taskbar");
}

/** ADR 0023: a new connection first, then the usual Connect step, then a short Fixed screen. */
export const REPAIR_STEPS: Step[] = ["repair", "connect", "fixed"];

export type Connect =
	| { kind: "loading" }
	| { kind: "preview"; preview: PreviewOk; applying: boolean }
	/** `stage` says whether reading the file or writing it failed. */
	| { kind: "failed"; stage: "preview" | "apply"; message: string }
	| { kind: "connected"; backup: string };

/** The test knock: not played yet, played and waiting for an answer, or not seen. */
export type TestKnock = "ready" | "played" | "missed";

/** `repair_server()`: not run yet, running, failed with the core's words, or done. */
export type Repair =
	| { kind: "idle" }
	| { kind: "working" }
	| { kind: "failed"; message: string }
	| { kind: "repaired" };

export interface Flow {
	steps: Step[];
	at: number;
	connect: Connect;
	test: TestKnock;
	repair: Repair;
}

export type Action =
	| { type: "next" }
	| { type: "back" }
	| { type: "previewed"; result: Preview }
	| { type: "apply" }
	| { type: "applied"; result: Written }
	| { type: "test"; result: TestKnock }
	| { type: "repair" }
	| { type: "repaired"; result: { ok: true } | Failure };

const begin = (steps: Step[]): Flow => ({
	steps,
	at: 0,
	connect: { kind: "loading" },
	test: "ready",
	repair: { kind: "idle" },
});

export const startFlow = (monitorCount: number): Flow =>
	begin(stepsFor(monitorCount));

export const startRepair = (): Flow => begin(REPAIR_STEPS);

export const stepOf = (f: Flow): Step => f.steps[f.at] ?? "welcome";

/**
 * Leaving Connect is only possible once the hooks are written (§8.2: write only after Confirm),
 * and leaving Repair only once the core has a new connection.
 */
export const canGoNext = (f: Flow): boolean =>
	f.at < f.steps.length - 1 &&
	(stepOf(f) !== "connect" || f.connect.kind === "connected") &&
	(stepOf(f) !== "repair" || f.repair.kind === "repaired");

export function flowReducer(f: Flow, a: Action): Flow {
	switch (a.type) {
		case "next":
			return canGoNext(f) ? move(f, f.at + 1) : f;
		case "back":
			return f.at > 0 ? move(f, f.at - 1) : f;
		case "previewed":
			if (stepOf(f) !== "connect" || f.connect.kind !== "loading") return f;
			return {
				...f,
				connect: a.result.ok
					? { kind: "preview", preview: a.result, applying: false }
					: { kind: "failed", stage: "preview", message: a.result.message },
			};
		case "apply":
			if (f.connect.kind !== "preview" || f.connect.applying) return f;
			return { ...f, connect: { ...f.connect, applying: true } };
		case "applied": {
			if (f.connect.kind !== "preview" || !f.connect.applying) return f;
			if (!a.result.ok)
				return {
					...f,
					connect: {
						kind: "failed",
						stage: "apply",
						message: a.result.message,
					},
				};
			const g: Flow = {
				...f,
				connect: { kind: "connected", backup: a.result.backup },
			};
			// Repair is two clicks (ADR 0023): after Confirm it goes straight to Fixed.
			return f.steps[f.at + 1] === "fixed" ? move(g, g.at + 1) : g;
		}
		case "test":
			return { ...f, test: a.result };
		case "repair":
			if (
				stepOf(f) !== "repair" ||
				f.repair.kind === "working" ||
				f.repair.kind === "repaired"
			)
				return f;
			return { ...f, repair: { kind: "working" } };
		case "repaired": {
			if (f.repair.kind !== "working") return f;
			if (!a.result.ok)
				return { ...f, repair: { kind: "failed", message: a.result.message } };
			// Claude Code's entries still point at the old connection: straight on to Connect.
			const done = { ...f, repair: { kind: "repaired" } as const };
			return move(done, done.at + 1);
		}
	}
}

function move(f: Flow, at: number): Flow {
	const step = f.steps[at];
	return {
		...f,
		at,
		// Coming back to Connect reads the file again (it may have been fixed meanwhile),
		// unless this run already connected.
		connect:
			step === "connect" && f.connect.kind !== "connected"
				? { kind: "loading" }
				: f.connect,
		test: step === "test" ? "ready" : f.test,
	};
}

// --- Settings validation, mirroring the core's limits ---

export const MINUTES_ERROR =
	"Enter a whole number of minutes from 1 to 1440 (1440 minutes is one day).";
export const TIME_ERROR =
	"Enter a full time, with both the hour and the minutes.";

export type Parsed<T> = { value: T } | { error: string };

/** Minutes are whole numbers from 1 to 1440. */
export function parseMinutes(text: string): Parsed<number> {
	const t = text.trim();
	const n = Number(t);
	return /^\d+$/.test(t) && n >= 1 && n <= 1440
		? { value: n }
		: { error: MINUTES_ERROR };
}

/** Quiet-hours times are HH:MM, 00:00 to 23:59. */
export function parseTime(text: string): Parsed<string> {
	const t = text.trim();
	return /^([01]\d|2[0-3]):[0-5]\d$/.test(t)
		? { value: t }
		: { error: TIME_ERROR };
}

/** The `update_settings` patch for one quiet-hours field: the whole object, so nothing is lost. */
export function quietHoursPatch(
	current: Settings["quietHours"],
	change: Partial<Settings["quietHours"]>,
): Partial<Settings> {
	return { quietHours: { ...current, ...change } };
}

// --- Taskbar choice ---

/** The main display is stored as "primary", so Knock follows it if the user changes it (§8.6). */
export const monitorValue = (m: Monitor): string =>
	m.primary ? "primary" : m.id;

/** Which listed display the `monitor` setting means, if any. */
export function selectedMonitor(
	setting: string,
	monitors: Monitor[],
): Monitor | undefined {
	return monitors.find((m) =>
		setting === "primary" ? m.primary : m.id === setting,
	);
}

/**
 * Windows' display names can repeat ("Generic PnP Monitor"), so the resolution always shows,
 * and displays alike in name and resolution are numbered by their place in the list.
 */
export function monitorLabel(
	m: Monitor,
	index: number,
	all: readonly Monitor[],
): string {
	const name = m.name.trim();
	const twin = all.some(
		(o) =>
			o !== m &&
			o.name.trim() === name &&
			o.width === m.width &&
			o.height === m.height,
	);
	const number = `Display ${index + 1}`;
	const title = !name ? number : twin ? `${number}: ${name}` : name;
	return `${title}${m.primary ? " (main display)" : ""}, ${m.width} × ${m.height}`;
}

// --- The exact change, line by line ---

export interface DiffLine {
	kind: "same" | "add" | "del";
	text: string;
}

const lines = (text: string) => (text === "" ? [] : text.split(/\r?\n/));

/** A line diff of the whole file. Common start and end are trimmed before the LCS. */
export function lineDiff(before: string, after: string): DiffLine[] {
	const a = lines(before);
	const b = lines(after);
	let start = 0;
	while (start < a.length && start < b.length && a[start] === b[start]) start++;
	let endA = a.length;
	let endB = b.length;
	while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
		endA--;
		endB--;
	}
	const same = (text: string): DiffLine => ({ kind: "same", text });
	return [
		...a.slice(0, start).map(same),
		...lcs(a.slice(start, endA), b.slice(start, endB)),
		...a.slice(endA).map(same),
	];
}

function lcs(a: string[], b: string[]): DiffLine[] {
	const n = a.length;
	const m = b.length;
	const del = (text: string): DiffLine => ({ kind: "del", text });
	const add = (text: string): DiffLine => ({ kind: "add", text });
	// ponytail: O(n·m) table; past a million cells the changed middle shows as removed then added.
	if (n * m > 1_000_000) return [...a.map(del), ...b.map(add)];
	const w = m + 1;
	const t = new Uint32Array((n + 1) * w);
	const at = (i: number, j: number) => t[i * w + j] as number;
	for (let i = n - 1; i >= 0; i--)
		for (let j = m - 1; j >= 0; j--)
			t[i * w + j] =
				a[i] === b[j]
					? at(i + 1, j + 1) + 1
					: Math.max(at(i + 1, j), at(i, j + 1));
	const out: DiffLine[] = [];
	let i = 0;
	let j = 0;
	while (i < n && j < m) {
		if (a[i] === b[j]) {
			out.push({ kind: "same", text: a[i] as string });
			i++;
			j++;
		} else if (at(i + 1, j) >= at(i, j + 1)) out.push(del(a[i++] as string));
		else out.push(add(b[j++] as string));
	}
	return [...out, ...a.slice(i).map(del), ...b.slice(j).map(add)];
}
