import { expect, test } from "vitest";
import type { SessionView, Settings } from "./bridge";
import { SAMPLES } from "./dev/samples";
import { createStripStore } from "./store";

const a: SessionView = SAMPLES.ready;
const b: SessionView = SAMPLES.working;
const settings = (size: Settings["size"]) => ({ size }) as Settings;

test("applies newer revs and replaces the whole session list", () => {
	const store = createStripStore();
	store.getState().applySessions({ rev: 1, sessions: [a, b] });
	store.getState().applySessions({ rev: 2, sessions: [b] });
	expect(store.getState()).toMatchObject({ rev: 2, sessions: [b] });
});

test("drops out-of-order and repeated revs", () => {
	const store = createStripStore();
	store.getState().applySessions({ rev: 5, sessions: [a] });
	store.getState().applySessions({ rev: 4, sessions: [b] });
	store.getState().applySessions({ rev: 5, sessions: [b] });
	expect(store.getState()).toMatchObject({ rev: 5, sessions: [a] });
});

test("keeps what the core sent, unchanged", () => {
	const store = createStripStore();
	const sessions = [a, b];
	store.getState().applySessions({ rev: 1, sessions });
	expect(store.getState().sessions).toBe(sessions);
});

test("initial load with a rev goes through the rev check", () => {
	const store = createStripStore();
	store.getState().applySessions({ rev: 3, sessions: [b] });
	store.getState().applyInitialSessions({ rev: 2, sessions: [a] });
	expect(store.getState().sessions).toEqual([b]);
	store.getState().applyInitialSessions({ rev: 4, sessions: [a] });
	expect(store.getState()).toMatchObject({ rev: 4, sessions: [a] });
});

test("an initial array without rev only fills an empty store, and any event wins", () => {
	const store = createStripStore();
	store.getState().applyInitialSessions([a]);
	expect(store.getState()).toMatchObject({ rev: null, sessions: [a] });
	store.getState().applySessions({ rev: 0, sessions: [b] });
	expect(store.getState().sessions).toEqual([b]);
	store.getState().applyInitialSessions([a]);
	expect(store.getState().sessions).toEqual([b]);
});

test("a failed initial load (undefined) changes nothing", () => {
	const store = createStripStore();
	store.getState().applyInitialSessions(undefined);
	store.getState().applyInitialSettings(undefined);
	expect(store.getState()).toMatchObject({
		rev: null,
		sessions: [],
		settings: null,
	});
});

test("settings-changed wins over a late get_settings result", () => {
	const store = createStripStore();
	store.getState().applySettings(settings("large"));
	store.getState().applyInitialSettings(settings("small"));
	expect(store.getState().settings?.size).toBe("large");
	store.getState().applySettings(settings("small"));
	expect(store.getState().settings?.size).toBe("small");
});
