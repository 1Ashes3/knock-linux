// "Hide the tag only": the user can close the "No progress for N min" chip of a stalled
// session. Overlay-local: nothing goes to the core, and the session itself is untouched
// (§8.5 `dismiss` stays Failed and ended only). The core still decides the state (§7.3);
// this only remembers which stall the user has already seen.

/** Session id -> the `since` of the stall whose tag was hidden. */
export type HiddenTags = Readonly<Record<string, string>>;

interface Stallable {
	id: string;
	state: string;
	since: string;
}

/** Hides `s`'s tag for its current stall. A session that isn't stalled is ignored. */
export function hideTag(hidden: HiddenTags, s: Stallable): HiddenTags {
	if (s.state !== "stalled") return hidden;
	return { ...hidden, [s.id]: s.since };
}

/** The tag of `s` is hidden: it is still in the stall the user closed the tag of. */
export function tagHidden(hidden: HiddenTags, s: Stallable): boolean {
	return s.state === "stalled" && hidden[s.id] === s.since;
}

/**
 * Forgets every session that is gone, has left stalled, or is in a new stall (a new
 * `since`), so the next stall shows its tag again. Returns `hidden` itself if nothing changed.
 */
export function pruneTags(
	hidden: HiddenTags,
	sessions: readonly Stallable[],
): HiddenTags {
	const ids = Object.keys(hidden);
	if (ids.length === 0) return hidden;
	const kept: Record<string, string> = {};
	let changed = false;
	for (const id of ids) {
		const s = sessions.find((x) => x.id === id);
		if (s && tagHidden(hidden, s)) kept[id] = hidden[id] as string;
		else changed = true;
	}
	return changed ? kept : hidden;
}
