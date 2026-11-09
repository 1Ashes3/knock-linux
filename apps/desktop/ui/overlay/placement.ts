// Where the strip lives (ADR 0037). With `placement` "desktop" the strip sits behind
// other windows; while a knock nobody has seen is waiting, the core raises it above
// them, and it shows only the sessions that knock. The core applies the same rule
// (`knocking` in main.rs) to pick the layer.
import type { SessionView } from "./bridge";

export type Placement = "top" | "desktop" | "hidden";

/** A knock nobody has seen yet (ADR 0016). */
export function knocking(s: SessionView): boolean {
	return s.needs !== null && !s.needs.seen;
}

/** The sessions the strip draws for `placement`. */
export function visibleSessions(
	sessions: SessionView[],
	placement: Placement | undefined,
): SessionView[] {
	if (placement !== "desktop") return sessions;
	const raised = sessions.filter(knocking);
	return raised.length > 0 ? raised : sessions;
}

/** The DOM events the overlay's entry fires when the pointer leaves or enters the strip (Linux). */
export const POINTER_LEFT = "knock:pointer-left";
export const POINTER_ENTERED = "knock:pointer-entered";
