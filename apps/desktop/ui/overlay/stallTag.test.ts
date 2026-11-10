import { expect, test } from "vitest";
import { metrics, tagDismissRect } from "./animals/layout";
import type { SessionView } from "./bridge";
import { SAMPLES } from "./dev/samples";
import { hideTag, pruneTags, tagHidden } from "./stallTag";
import { createStripStore } from "./store";

const stalled: SessionView = {
	...SAMPLES.stalled,
	since: "2026-11-01T12:00:00Z",
};
const working: SessionView = { ...stalled, state: "working", tier: "working" };
const stalledAgain: SessionView = { ...stalled, since: "2026-11-01T12:30:00Z" };
const other: SessionView = SAMPLES.ready;

test("a hidden tag stays hidden while the session stays in that stall", () => {
	const h = hideTag({}, stalled);
	expect(tagHidden(h, stalled)).toBe(true);
	expect(pruneTags(h, [stalled, other])).toBe(h);
	expect(tagHidden(pruneTags(h, [stalled]), stalled)).toBe(true);
});

test("only a stalled session's tag can be hidden", () => {
	expect(hideTag({}, working)).toEqual({});
	expect(tagHidden({ [working.id]: working.since }, working)).toBe(false);
});

test("leaving stalled forgets it, so the next stall shows the tag again", () => {
	let h = hideTag({}, stalled);
	h = pruneTags(h, [working]);
	expect(h).toEqual({});
	expect(tagHidden(h, stalledAgain)).toBe(false);
});

test("a new stall (a new since) shows the tag, even if the in-between state was missed", () => {
	const h = hideTag({}, stalled);
	expect(tagHidden(h, stalledAgain)).toBe(false);
	expect(pruneTags(h, [stalledAgain])).toEqual({});
});

test("a session that went away is forgotten", () => {
	const h = hideTag(hideTag({}, stalled), { ...stalled, id: "x" });
	expect(pruneTags(h, [stalled])).toEqual({ [stalled.id]: stalled.since });
	expect(pruneTags(h, [])).toEqual({});
});

test("store: hide, keep through updates, forget after progress", () => {
	const store = createStripStore();
	const { applySessions } = store.getState();
	applySessions({ rev: 1, sessions: [stalled, other] });
	store.getState().hideStallTag(stalled.id);
	expect(tagHidden(store.getState().hiddenTags, stalled)).toBe(true);
	applySessions({ rev: 2, sessions: [stalled, other] });
	expect(tagHidden(store.getState().hiddenTags, stalled)).toBe(true);
	applySessions({ rev: 3, sessions: [working, other] });
	expect(store.getState().hiddenTags).toEqual({});
	applySessions({ rev: 4, sessions: [stalledAgain, other] });
	expect(tagHidden(store.getState().hiddenTags, stalledAgain)).toBe(false);
});

test("store: hiding sends nothing and leaves the sessions untouched", () => {
	const store = createStripStore();
	const sessions = [stalled];
	store.getState().applySessions({ rev: 1, sessions });
	store.getState().hideStallTag(stalled.id);
	store.getState().hideStallTag("unknown");
	expect(store.getState().sessions).toBe(sessions);
	expect(Object.keys(store.getState().hiddenTags)).toEqual([stalled.id]);
});

test("the tag's x sits on the chip's top-right corner, inside the strip", () => {
	const m = metrics("medium");
	const d = Math.round(m.s * 0.25);
	const r = tagDismissRect({ x: 100, y: 20, width: 80 }, 10, 384, m);
	expect(r).toEqual({ x: 180 - d / 2, y: 30 - d / 2, width: d, height: d });
	// At the strip's right and top edges it is pulled inside.
	const edge = tagDismissRect({ x: 330, y: 0, width: 54 }, 0, 384, m);
	expect(edge).toEqual({ x: 384 - d, y: 0, width: d, height: d });
});
