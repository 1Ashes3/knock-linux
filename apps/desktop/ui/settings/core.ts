// The setup and settings commands (SPEC.md §8.5). Result shapes are fixed by the Phase 2
// brief; the core is the only source of truth (§7.3).

export interface SetupStatus {
	firstRun: boolean;
	hooksInstalled: boolean;
	serverOk: boolean;
	/** Why the server can't start, in plain words; null when it's fine (ADR 0023). */
	serverError: string | null;
	monitorCount: number;
}

export interface Monitor {
	id: string;
	name: string;
	primary: boolean;
	width: number;
	height: number;
}

export type Failure = { ok: false; message: string };
export type PreviewOk = {
	ok: true;
	path: string;
	before: string;
	after: string;
};
export type Preview = PreviewOk | Failure;
export type Written = { ok: true; backup: string } | Failure;

export interface Check {
	id: string;
	label: string;
	ok: boolean;
	detail: string;
}

const UNKNOWN = "Something went wrong inside Knock. Please try again.";

/** Unlike `bridge.call`, this rejects with the core's error, so the screen can show it. */
export async function invoke<T>(
	cmd: string,
	args?: Record<string, unknown>,
): Promise<T> {
	const run = globalThis.__TAURI__?.core.invoke;
	if (!run) throw new Error("Knock isn't running, so this can't be done.");
	return (await run(cmd, args)) as T;
}

/** `invoke` with a raw body: Tauri hands the command the bytes as they are. */
export async function invokeBytes<T>(
	cmd: string,
	bytes: Uint8Array,
): Promise<T> {
	const run = globalThis.__TAURI__?.core.invoke;
	if (!run) throw new Error("Knock isn't running, so this can't be done.");
	return (await run(cmd, bytes)) as T;
}

/** The core's error, as text. Tauri rejects with whatever the command returned as Err. */
export function errorText(e: unknown): string {
	if (typeof e === "string" && e) return e;
	if (e && typeof e === "object" && "message" in e && e.message)
		return String(e.message);
	return UNKNOWN;
}

/** Runs a command that answers `{ ok, ... }`; a thrown error or odd reply becomes a Failure. */
export async function invokeResult<T extends { ok: true }>(
	cmd: string,
): Promise<T | Failure> {
	try {
		const r = await invoke<T | Failure | undefined>(cmd);
		if (r && typeof r === "object" && "ok" in r)
			return r.ok ? r : { ok: false, message: r.message || UNKNOWN };
		return { ok: false, message: UNKNOWN };
	} catch (e) {
		return { ok: false, message: errorText(e) };
	}
}

/** Closes this window. Needs the window's `core:window:allow-close` permission. */
export async function closeWindow(): Promise<boolean> {
	const tauri = globalThis.__TAURI__ as
		| { window?: { getCurrentWindow(): { close(): Promise<void> } } }
		| undefined;
	try {
		await tauri?.window?.getCurrentWindow().close();
		return Boolean(tauri?.window);
	} catch {
		return false;
	}
}
