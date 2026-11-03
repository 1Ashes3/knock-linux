// Dev only: a fake core behind `window.__TAURI__`, so the strip runs in a plain browser.
// Opened through dev/mock.html, which Vite never builds, so it is never shipped.
//
// URL parameters:
//   show=permission,ready,...  sample sessions (dev/samples.ts), first nearest the clock; default all
//   size=small|medium|large    strip size; default medium
//   taskbar=light|dark         background; default dark
//   mode=plain|animals         settings.mode; default plain
//   animal=<id>                every sample's animal; default fox
//
// Tests drive changes through `window.__knockMock` (see the end of this file).
import type { SessionsChanged, SessionView, Settings } from "../bridge";
import { SAMPLE_KEYS, SAMPLES, type SampleKey } from "./samples";

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
};

let rev = 1;
const animal = params.get("animal");
let sessions: SessionView[] = keys.map((k) => ({
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
	set_hit_regions: () => undefined,
	request_strip_size: () => undefined,
};

globalThis.__TAURI__ = {
	core: {
		invoke: async (cmd, args = {}) => {
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
};
(globalThis as { __knockMock?: typeof mock }).__knockMock = mock;

await import("../main");
