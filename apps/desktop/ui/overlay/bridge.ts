// The core <-> UI interface (SPEC.md §8.5) and settings (§8.6). The core is the only
// source of truth (§7.3): these types mirror what it sends, nothing more.

export type Tier = "needs_you" | "ready" | "working" | "resting";

/** §8.4. Some are unused in v1 but stay in the type. */
export type State =
	| "needs_input"
	| "failed"
	| "stalled"
	| "blocked_fixable"
	| "ready"
	| "working"
	| "waiting_agent"
	| "communicating"
	| "idle"
	| "ended_unexpectedly"
	| "paused"
	| "cancelled";

export interface Needs {
	kind: "permission" | "answer";
	/** The tool, for a permission; null for a question or when unknown. */
	label: string | null;
	/** The user clicked or jumped to it (ADR 0016). */
	seen: boolean;
}

export interface SessionView {
	id: string;
	project: string;
	tier: Tier;
	state: State;
	/** ISO 8601: when `state` was entered. */
	since: string;
	/** Tool category (§8.3) or null. */
	activity: string | null;
	needs: Needs | null;
	precision: "exact_window" | "app_window" | "none";
	animal: string;
	simulated: boolean;
}

export interface SessionsChanged {
	rev: number;
	sessions: SessionView[];
}

/** §8.5 `away-summary`: what changed while the user was away (FR-9, ADR 0034). */
export interface AwaySummary {
	/** ISO 8601: when the idle period started. */
	since: string;
	items: AwayItem[];
}

export interface AwayItem {
	id: string;
	project: string;
	/** The session's status when the user came back. */
	outcome: "ready" | "failed" | "needs_you";
}

export type Size = "small" | "medium" | "large";

export interface Settings {
	version: number;
	monitor: string;
	mode: "animals" | "plain";
	size: Size;
	sounds: boolean;
	stallMinutes: number;
	escalationMinutes: number;
	notifyOnStall: boolean;
	pauseNotifications: boolean;
	quietHours: { enabled: boolean; start: string; end: string };
	launchAtSignIn: boolean | null;
	animalOverrides: Record<string, string>;
	/** ADR 0037: where the strip lives. */
	placement: "top" | "desktop" | "hidden";
	/** ADR 0037: seconds a hover card stays after the mouse leaves, fading out. */
	cardFadeSeconds: number;
	/** ADR 0038: the sprite pack sessions get characters from; "classic" is Knock's own. */
	pack: string;
	/** ADR 0038: built-in packs the user removed; hidden and never used. */
	hiddenPacks: string[];
}

/** ADR 0038: one character of a pack, as `list_packs` and `get_pack` give it. */
export interface PackCharacterInfo {
	id: string;
	displayName: string;
	kind: "rig" | "sheet";
}

/** ADR 0038: one entry of `list_packs`. */
export interface PackSummary {
	id: string;
	displayName: string;
	author?: string;
	license?: string;
	description?: string;
	builtin: boolean;
	valid: boolean;
	/** Why the pack can't be used; empty when valid. */
	errors: string[];
	characters: PackCharacterInfo[];
	/** The pack draws its own overflow den (`den.svg`). */
	den: boolean;
}

/** ADR 0038: `get_pack`: an installed pack's rigs, sheets and clips, as the pack wrote them. */
export interface PackData {
	id: string;
	characters: (PackCharacterInfo & { rig?: unknown; sheet?: unknown })[];
	clips: Record<string, unknown>;
	den: boolean;
}

/** ADR 0003: logical px from the strip's top-left. */
export interface HitRegion {
	x: number;
	y: number;
	width: number;
	height: number;
}

const logged = new Set<string>();

function logOnce(key: string, ...args: unknown[]): void {
	if (logged.has(key)) return;
	logged.add(key);
	console.warn(...args);
}

/** Invokes a §8.5 command. Without Tauri, or on failure, logs once per command and resolves undefined. */
export async function call(
	cmd: string,
	args?: Record<string, unknown>,
): Promise<unknown> {
	const invoke = globalThis.__TAURI__?.core.invoke;
	if (!invoke) {
		logOnce(cmd, `Tauri not available; skipping ${cmd}.`);
		return undefined;
	}
	try {
		return await invoke(cmd, args);
	} catch (e) {
		logOnce(cmd, `${cmd} failed`, e);
		return undefined;
	}
}

/** Subscribes to a §8.5 event. Without Tauri, logs once and does nothing. */
export async function listen<T>(
	event: string,
	handler: (payload: T) => void,
): Promise<void> {
	const on = globalThis.__TAURI__?.event.listen;
	if (!on) {
		logOnce(event, `Tauri not available; not listening for ${event}.`);
		return;
	}
	await on<T>(event, (e) => handler(e.payload));
}
