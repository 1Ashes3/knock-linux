/** `SessionView` (§8.5). Spike C: only `id`, `project` and `precision` are guaranteed. */
export interface SessionView {
	id: string;
	project: string;
	precision: "exact_window" | "app_window" | "none";
	tier?: string;
	state?: string;
	since?: string;
	activity?: string;
	needs?: { kind: string; label: string };
	animal?: string;
	simulated?: boolean;
}

/** The core sorts most recent first, so the first entry is the target. */
export function firstSession(sessions: SessionView[]): SessionView | undefined {
	return sessions[0];
}

export function sessionLabel(session: SessionView | undefined): string {
	return session ? `${session.project} · ${session.precision}` : "no sessions";
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

export async function getSessions(): Promise<SessionView[]> {
	const result = await call("get_sessions");
	return Array.isArray(result) ? (result as SessionView[]) : [];
}
