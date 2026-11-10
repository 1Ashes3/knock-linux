// Dev only: a fake core behind `window.__TAURI__`, so the strip runs in a plain browser.
// Opened through dev/mock.html, which Vite never builds, so it is never shipped.
//
// URL parameters:
//   show=permission,ready,...  sample sessions (dev/samples.ts), first nearest the clock; default all
//   size=small|medium|large    strip size; default medium
//   taskbar=light|dark         background; default dark
//   mode=plain|animals         settings.mode; default plain
//   animal=<id>                every sample's animal; default fox
//   away=<n>                   sends an `away-summary` with n items once loaded
//   set=eight-den              the end of `knock sim eight-den` instead of samples (§12.3)
//
// Tests drive changes through `window.__knockMock` (see the end of this file).
import type {
	AwaySummary,
	SessionsChanged,
	SessionView,
	Settings,
} from "../bridge";
import {
	awaySample,
	EIGHT_DEN,
	SAMPLE_KEYS,
	SAMPLES,
	type SampleKey,
} from "./samples";

const params = new URLSearchParams(location.search);
const keys = (params.get("show")?.split(",") ?? SAMPLE_KEYS).filter(
	(k): k is SampleKey => k in SAMPLES,
);
document.body.className = params.get("taskbar") === "light" ? "light" : "dark";

const settings: Settings = {
	version: 1,
	monitor: "primary",
	mode: params.get("mode") === "animals" ? "animals" : "plain",
	size: (params.get("size") as Settings["size"] | null) ?? "medium",
	sounds: false,
	stallMinutes: 10,
	escalationMinutes: 5,
	notifyOnStall: false,
	pauseNotifications: false,
	quietHours: { enabled: false, start: "22:00", end: "08:00" },
	launchAtSignIn: null,
	animalOverrides: {},
	placement: "top",
	cardFadeSeconds: 3,
	pack: "classic",
	hiddenPacks: [],
	idleMotion: true,
};

let rev = 1;
const animal = params.get("animal");
let sessions: SessionView[] =
	params.get("set") === "eight-den"
		? structuredClone(EIGHT_DEN)
		: keys.map((k) => ({
				...structuredClone(SAMPLES[k]),
				...(animal ? { animal } : {}),
			}));
const handlers = new Map<string, ((e: { payload: unknown }) => void)[]>();

function emit(event: string, payload: unknown): void {
	for (const h of handlers.get(event) ?? []) h({ payload });
}

function update(id: string, change: (s: SessionView) => SessionView | null) {
	sessions = sessions.flatMap((s) => {
		if (s.id !== id) return [s];
		const next = change(s);
		return next ? [next] : [];
	});
	emit("sessions-changed", { rev: ++rev, sessions } satisfies SessionsChanged);
}

const log = document.getElementById("log") as HTMLElement;
const root = document.getElementById("root") as HTMLElement;

/** §9.2, as the core does it (src-tauri/src/strip.rs `clamp`): at most 6 slots wide. */
const SLOT: Record<Settings["size"], number> = {
	small: 48,
	medium: 64,
	large: 80,
};
const TASKBAR_PX = 48;
const RIGHT_PX = 8;

/**
 * Clips the overlay to the window the core would give it: right edge 8 px from the
 * screen edge, bottom on the taskbar, width capped at 6 slots and height at the work
 * area. Anything the UI draws outside it is cut off, as in the real window.
 */
function placeWindow(width: number, height: number): void {
	const w = Math.min(width, 6 * SLOT[settings.size]);
	const h = Math.min(height, innerHeight - TASKBAR_PX);
	const top = innerHeight - TASKBAR_PX - h;
	const left = innerWidth - RIGHT_PX - w;
	root.style.clipPath = `inset(${top}px ${RIGHT_PX}px ${TASKBAR_PX}px ${left}px)`;
	root.dataset.window = JSON.stringify({
		x: left,
		y: top,
		width: w,
		height: h,
	});
}

// Roughly what the core does for these commands (§8.4), so clicks visibly do something.
const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
	get_sessions: () => ({ rev, sessions }),
	get_settings: () => settings,
	jump_to_session: ({ id }) =>
		update(id as string, (s) =>
			s.state === "ready"
				? { ...s, tier: "resting", state: "idle" }
				: s.needs
					? { ...s, needs: { ...s.needs, seen: true } }
					: s,
		),
	acknowledge: ({ id }) =>
		update(id as string, (s) =>
			s.state === "ready" ? { ...s, tier: "resting", state: "idle" } : s,
		),
	dismiss: ({ id }) =>
		update(id as string, (s) =>
			s.state === "failed" || s.state === "ended_unexpectedly" ? null : s,
		),
	dismiss_away_summary: () => undefined,
	set_hit_regions: () => undefined,
	request_strip_size: ({ width, height }) =>
		placeWindow(width as number, height as number),
};

globalThis.__TAURI__ = {
	core: {
		invoke: async (cmd, raw = {}) => {
			const args = raw instanceof Uint8Array ? { bytes: raw } : raw;
			log.textContent = `${cmd} ${JSON.stringify(args)}\n${log.textContent}`;
			const run = commands[cmd];
			if (!run) throw new Error(`mock: unknown command ${cmd}`);
			return run(args);
		},
	},
	event: {
		listen: async <T>(event: string, h: (e: { payload: T }) => void) => {
			const list = handlers.get(event) ?? [];
			list.push(h as (e: { payload: unknown }) => void);
			handlers.set(event, list);
			return () => {};
		},
	},
};

/** For tests: change what the fake core sends, as the real one would. */
const mock = {
	/** Replaces fields of one session, or adds a sample session. */
	set(id: string, patch: Partial<SessionView>): void {
		if (!sessions.some((s) => s.id === id) && id in SAMPLES)
			sessions = [...sessions, structuredClone(SAMPLES[id as SampleKey])];
		update(id, (s) => ({ ...s, ...patch }));
	},
	remove(id: string): void {
		update(id, () => null);
	},
	settings(patch: Partial<Settings>): void {
		Object.assign(settings, patch);
		emit("settings-changed", { settings: { ...settings } });
	},
	/** Sends `away-summary`: a given one, or a sample with `n` items (dev/samples.ts). */
	away(summary: AwaySummary | number = 3): void {
		emit(
			"away-summary",
			typeof summary === "number" ? awaySample(summary) : summary,
		);
	},
};
(globalThis as { __knockMock?: typeof mock }).__knockMock = mock;

await import("../main"); // resolves once it listens and has loaded
if (params.has("away")) mock.away(Number(params.get("away")) || 3);
