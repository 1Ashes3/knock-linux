// Where the strip lives (ADR 0037). With `placement` "desktop" the strip sits behind
// other windows; while a session needs a look (an unseen knock, or a fresh finish), the
// core raises it above them, and it shows only those sessions. The core applies the
// same rule (`rises` in main.rs) to pick the layer.
import { useEffect, useMemo, useState } from "react";
import type { SessionView } from "./bridge";

/** How long a finished session keeps the strip up if nobody clicks it. */
export const READY_RISE_MS = 2 * 60_000;

export type Placement = "top" | "desktop" | "hidden";

/** A knock nobody has seen yet (ADR 0016). */
export function knocking(s: SessionView): boolean {
	return s.needs !== null && !s.needs.seen;
}

/** When a finished session stops raising the strip; null if it isn't finished. */
function readyUntil(s: SessionView): number | null {
	return s.state === "ready" ? Date.parse(s.since) + READY_RISE_MS : null;
}

/** The strip rises for `s` at `now`: an unseen knock, or a finish under 2 minutes old. */
export function rises(s: SessionView, now: number): boolean {
	const until = readyUntil(s);
	return knocking(s) || (until !== null && now < until);
}

/** The sessions the strip draws for `placement` at `now`. */
export function visibleSessions(
	sessions: SessionView[],
	placement: Placement | undefined,
	now: number,
): SessionView[] {
	if (placement !== "desktop") return sessions;
	const raised = sessions.filter((s) => rises(s, now));
	return raised.length > 0 ? raised : sessions;
}

/** The next time `visibleSessions` changes with no new event; Infinity if never. */
export function nextChange(sessions: SessionView[], now: number): number {
	let next = Number.POSITIVE_INFINITY;
	for (const s of sessions) {
		const until = readyUntil(s);
		if (until !== null && until > now && until < next) next = until;
	}
	return next;
}

/** [`visibleSessions`], drawn again when a finish stops raising the strip. */
export function useVisibleSessions(
	sessions: SessionView[],
	placement: Placement | undefined,
): SessionView[] {
	const [tick, setTick] = useState(0);
	// biome-ignore lint/correctness/useExhaustiveDependencies: `tick` re-evaluates at the expiry
	const [shown, next] = useMemo(() => {
		const now = Date.now();
		return [
			visibleSessions(sessions, placement, now),
			nextChange(sessions, now),
		];
	}, [sessions, placement, tick]);
	useEffect(() => {
		if (!Number.isFinite(next)) return;
		const t = setTimeout(() => setTick((n) => n + 1), next - Date.now() + 50);
		return () => clearTimeout(t);
	}, [next]);
	return shown;
}

/** The DOM events the overlay's entry fires when the pointer leaves or enters the strip (Linux). */
export const POINTER_LEFT = "knock:pointer-left";
export const POINTER_ENTERED = "knock:pointer-entered";
