// Dev only: a fake core behind `window.__TAURI__`, so setup and Settings run in a plain
// browser. Opened through dev/mock.html, which Vite never builds, so it is never shipped.
//
// URL parameters:
//   setup=1                 start the setup flow (as the core's `?setup=1`)
//   repair=1                start the Repair flow (as the core's `?repair=1`, ADR 0023);
//                           without server=fail it shows the fallback explanation
//   server=fail             the server can't start: setup_status has a serverError
//   fix=fail                repair_server fails
//   monitors=1|2|3          displays connected; default 3. `window.__setMonitors(n)` changes
//                           it later, as plugging a display in or out would
//   monitor=<id>            the saved taskbar choice; default "primary"
//   installed=1             Knock's hooks are already in the Claude Code settings file
//   preview=invalid         the settings file isn't valid JSON
//   apply=fail              writing the file fails
//   save=fail               update_settings rejects every change
//   knock=fail              play_test_knock rejects
//   empty=1                 the settings file doesn't exist yet, so there is no backup
// Every call is logged under the page and kept in `window.__calls`.
import type { Settings } from "../../overlay/bridge";
import type { Check, Monitor } from "../core";

const q = new URLSearchParams(location.search);
const calls: [string, unknown][] = [];
(globalThis as unknown as { __calls: typeof calls }).__calls = calls;

let settings: Settings = {
	version: 1,
	monitor: q.get("monitor") ?? "primary",
	mode: "plain",
	size: "medium",
	sounds: false,
	stallMinutes: 10,
	escalationMinutes: 5,
	notifyOnStall: false,
	pauseNotifications: false,
	quietHours: { enabled: false, start: "22:00", end: "08:00" },
	launchAtSignIn: null,
	animalOverrides: {},
};
let installed = q.get("installed") === "1";
const SERVER_ERROR =
	"Another program on this PC took the connection Knock uses to hear from Claude Code.";
let serverError: string | null =
	q.get("server") === "fail" ? SERVER_ERROR : null;
// The port in Claude Code's entries, and the one the server listens on.
let hookPort = 47615;
let port = 47615;

const ALL_MONITORS: Monitor[] = [
	{
		id: "monitor-DEL41A8-1",
		name: "DELL U2720Q",
		primary: true,
		width: 2560,
		height: 1440,
	},
	// Windows names many displays alike; the labels number these two.
	{
		id: "monitor-GSM5B7F-2",
		name: "Generic PnP Monitor",
		primary: false,
		width: 1920,
		height: 1080,
	},
	{
		id: "monitor-GSM5B7F-3",
		name: "Generic PnP Monitor",
		primary: false,
		width: 1920,
		height: 1080,
	},
];
let monitors = ALL_MONITORS.slice(0, Number(q.get("monitors") ?? 3));
(globalThis as unknown as { __setMonitors(n: number): void }).__setMonitors = (
	n,
) => {
	monitors = ALL_MONITORS.slice(0, n);
};

const PATH = String.raw`C:\Users\Sam\.claude\settings.json`;
const userFile = {
	model: "opus",
	permissions: { allow: ["Bash(git status)"] },
};
const handler = (p: number) => ({
	type: "http",
	url: `http://127.0.0.1:${p}/v1/hooks/claude-code`,
	headers: { "X-Knock-Token": "••••" },
	timeout: 1,
});
const TOOL = ["PreToolUse", "PostToolUse", "PermissionRequest"];
const EVENTS = [
	"UserPromptSubmit",
	...TOOL,
	"Notification",
	"Stop",
	"StopFailure",
	"SubagentStart",
	"SubagentStop",
	"PreCompact",
	"SessionEnd",
];
const withHooks = (p: number) => ({
	...userFile,
	hooks: Object.fromEntries(
		EVENTS.map((e) => [
			e,
			[
				TOOL.includes(e)
					? { matcher: "*", hooks: [handler(p)] }
					: { hooks: [handler(p)] },
			],
		]),
	),
});
const text = (o: unknown) => `${JSON.stringify(o, null, 2)}\n`;
const before = () =>
	q.get("empty") === "1" && !installed
		? ""
		: text(installed ? withHooks(hookPort) : userFile);
const BACKUP = String.raw`C:\Users\Sam\AppData\Roaming\Knock\backups\settings.20261101-142530.json`;

const checks = (): Check[] => [
	installed
		? {
				id: "hooks",
				label: "Knock is connected to Claude Code",
				ok: true,
				detail: "Claude Code's settings file has all of Knock's entries.",
			}
		: {
				id: "hooks",
				label: "Knock isn't connected to Claude Code",
				ok: false,
				detail: "Run setup to connect it.",
			},
	serverError === null
		? {
				id: "server",
				label: "Knock is listening for Claude Code",
				ok: true,
				detail: "Claude Code can reach Knock on this PC.",
			}
		: {
				id: "server",
				label: "Knock can't listen for Claude Code",
				ok: false,
				detail: "Use Repair to pick a new connection.",
			},
	{
		id: "last",
		label: "Last heard from a session",
		ok: true,
		detail: "2 minutes ago",
	},
];

const handlers = new Map<string, ((e: { payload: unknown }) => void)[]>();
const emit = (event: string, payload: unknown) => {
	for (const h of handlers.get(event) ?? []) h({ payload });
};

const MINUTES = ["stallMinutes", "escalationMinutes"] as const;
const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
	get_settings: () => settings,
	update_settings: ({ patch }) => {
		if (q.get("save") === "fail")
			throw "Knock couldn't save your settings because the settings file is in use. Try again in a moment.";
		const p = patch as Partial<Settings>;
		for (const k of MINUTES)
			if (
				k in p &&
				!(
					Number.isInteger(p[k]) &&
					(p[k] as number) >= 1 &&
					(p[k] as number) <= 1440
				)
			)
				throw "Minutes must be a whole number from 1 to 1440.";
		settings = { ...settings, ...p };
		emit("settings-changed", { settings });
	},
	setup_status: () => ({
		firstRun: settings.launchAtSignIn === null,
		hooksInstalled: installed,
		serverOk: serverError === null,
		serverError,
		monitorCount: monitors.length,
	}),
	repair_server: () => {
		if (q.get("fix") === "fail")
			return {
				ok: false,
				message:
					"Knock couldn't find a free connection. Restart your PC, then open Knock and try again.",
			};
		port = 51874;
		serverError = null;
		return { ok: true };
	},
	setup_list_monitors: () => monitors,
	setup_preview_hooks: () =>
		q.get("preview") === "invalid"
			? {
					ok: false,
					message:
						"Your Claude Code settings file has a mistake on line 4, so it can't be read safely. Knock won't change it. Fix the mistake (or ask someone to help), then run setup again.",
				}
			: {
					ok: true,
					path: PATH,
					before: before(),
					after: text(withHooks(port)),
				},
	setup_apply_hooks: () => {
		if (q.get("apply") === "fail")
			return {
				ok: false,
				message:
					"Knock couldn't save the file because another program is using it. Close other programs that might have it open, then try again.",
			};
		const backup = before() === "" ? "" : BACKUP;
		installed = true;
		hookPort = port;
		return { ok: true, backup };
	},
	setup_remove_hooks: () => {
		installed = false;
		return { ok: true, backup: BACKUP };
	},
	run_health_check: () => checks(),
	play_test_knock: () => {
		if (q.get("knock") === "fail") throw "The strip isn't showing.";
	},
};

const log = document.getElementById("log") as HTMLElement;
globalThis.__TAURI__ = {
	core: {
		invoke: async (cmd, args = {}) => {
			calls.push([cmd, args]);
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

await import("../main");
