import { afterEach, describe, expect, test, vi } from "vitest";
import { CLIPS } from "./animals/art";
import {
	characterRect,
	introMetrics,
	MAX_SLOTS,
	metrics,
	rowHeight,
	slotTop,
	slotX,
} from "./animals/layout";
import { EMPTY_MEMORY, step } from "./animals/model";
import type { SessionView, Settings } from "./bridge";
import { SAMPLES } from "./dev/samples";
import {
	DEMO_ID,
	DEMO_STATES,
	DEMO_TOOL,
	type DemoCharacter,
	demoAt,
	demoEnd,
	INTRO_MS,
	interrupts,
	PICKED,
	pick,
	STATE_MS,
	startDemo,
} from "./introDemo";
import { stateLabel } from "./pill";
import { createStripStore } from "./store";

const chars = (n: number): DemoCharacter[] =>
	Array.from({ length: n }, (_, i) => ({
		id: `c${i}`,
		displayName: `Character ${i}`,
	}));

/** A seeded generator, so picks are repeatable. */
function seeded(seed: number): () => number {
	let x = seed;
	return () => {
		x = (x * 1103515245 + 12345) % 2 ** 31;
		return x / 2 ** 31;
	};
}

const T0 = 1_000_000;

describe("pick", () => {
	test("4 different characters out of many", () => {
		for (let seed = 1; seed < 50; seed++) {
			const all = chars(10);
			const got = pick(all, PICKED, seeded(seed));
			expect(got).toHaveLength(4);
			expect(new Set(got.map((c) => c.id)).size).toBe(4);
			for (const c of got) expect(all).toContain(c);
		}
	});

	test("all of them when the pack has fewer than 4", () => {
		for (const n of [1, 2, 3, 4]) {
			const got = pick(chars(n), PICKED, seeded(7));
			expect(got.map((c) => c.id).sort()).toEqual(
				chars(n)
					.map((c) => c.id)
					.sort(),
			);
		}
	});

	test("random: every character can be picked", () => {
		const seen = new Set<string>();
		for (let seed = 1; seed < 200; seed++)
			for (const c of pick(chars(10), PICKED, seeded(seed))) seen.add(c.id);
		expect(seen.size).toBe(10);
	});

	test("does not change the list it was given", () => {
		const all = chars(6);
		const copy = [...all];
		pick(all, PICKED, seeded(3));
		expect(all).toEqual(copy);
	});
});

describe("timeline", () => {
	const demo = startDemo(chars(7), T0, [], seeded(1));
	if (!demo) throw new Error("no demo");

	test("no characters: no demo", () => {
		expect(startDemo([], T0, [])).toBeNull();
	});

	test("the introduction: every character asleep, for 3 s", () => {
		for (const t of [0, 1500, INTRO_MS - 1]) {
			const f = demoAt(demo, T0 + t);
			expect(f.phase).toBe("intro");
			expect(f.sessions.map((s) => s.animal)).toEqual(
				demo.all.map((c) => c.id),
			);
			for (const s of f.sessions) {
				expect(s.state).toBe("idle");
				expect(s.tier).toBe("resting");
			}
			expect(f.nextAt).toBe(T0 + INTRO_MS);
		}
	});

	test("then the picked ones go through sleep, work, knock, failed, done", () => {
		const seen: string[] = [];
		for (let i = 0; i < DEMO_STATES.length; i++) {
			const at = T0 + INTRO_MS + i * STATE_MS;
			for (const t of [at, at + STATE_MS - 1]) {
				const f = demoAt(demo, t);
				expect(f.phase).toBe("states");
				expect(f.sessions.map((s) => s.animal)).toEqual(
					demo.picked.map((c) => c.id),
				);
				expect(new Set(f.sessions.map((s) => s.state)).size).toBe(1);
				expect(f.nextAt).toBe(at + STATE_MS);
				for (const s of f.sessions) expect(Date.parse(s.since)).toBe(at);
			}
			seen.push(demoAt(demo, at).sessions[0]?.state ?? "");
		}
		expect(seen).toEqual(["idle", "working", "needs_input", "failed", "ready"]);
	});

	test("each state has the tier and the chip text the real one has", () => {
		const at = (i: number) => demoAt(demo, T0 + INTRO_MS + i * STATE_MS);
		const one = (i: number) => at(i).sessions[0] as SessionView;
		expect(one(1).tier).toBe("working");
		expect(one(2).tier).toBe("needs_you");
		expect(one(2).needs).toEqual({
			kind: "permission",
			label: DEMO_TOOL,
			seen: false,
		});
		expect(stateLabel(one(2), 0, 10)).toBe(`Needs permission: ${DEMO_TOOL}`);
		expect(one(3).tier).toBe("needs_you");
		expect(stateLabel(one(3), 0, 10)).toBe("Stopped with an error");
		expect(one(4).tier).toBe("ready");
		expect(stateLabel(one(4), 0, 10)).toBe("Done, ready to review");
	});

	test("demo sprites are named after the character and marked as made up", () => {
		for (const s of demoAt(demo, T0).sessions) {
			expect(s.id.startsWith(DEMO_ID)).toBe(true);
			expect(s.project).toBe(
				demo.all.find((c) => c.id === s.animal)?.displayName,
			);
			expect(s.simulated).toBe(true);
			expect(s.precision).toBe("none");
		}
	});

	test("then it is over: nothing to draw", () => {
		const end = demoEnd(demo);
		expect(end).toBe(T0 + INTRO_MS + 5 * STATE_MS);
		expect(demoAt(demo, end - 1).phase).toBe("states");
		const f = demoAt(demo, end);
		expect(f.phase).toBe("over");
		expect(f.sessions).toEqual([]);
		expect(f.nextAt).toBe(Number.POSITIVE_INFINITY);
	});

	test("a pack of 2: both go through the states", () => {
		const d = startDemo(chars(2), T0, [], seeded(2));
		if (!d) throw new Error("no demo");
		expect(demoAt(d, T0 + INTRO_MS).sessions).toHaveLength(2);
	});
});

describe("safety stop", () => {
	const knock: SessionView = { ...SAMPLES.permission, id: "real-1" };
	const working: SessionView = { ...SAMPLES.working, id: "real-2" };

	test("a real session that starts needing the user stops it", () => {
		const demo = startDemo(chars(4), T0, [working]);
		if (!demo) throw new Error("no demo");
		expect(interrupts(demo, [working])).toBe(false);
		expect(interrupts(demo, [knock])).toBe(true);
		expect(
			interrupts(demo, [{ ...working, state: "failed", tier: "needs_you" }]),
		).toBe(true);
	});

	test("a knock already seen does not", () => {
		const demo = startDemo(chars(4), T0, []);
		if (!demo) throw new Error("no demo");
		const seen = { ...knock, needs: { ...knock.needs, seen: true } };
		expect(interrupts(demo, [seen as SessionView])).toBe(false);
	});

	test("one already needing the user when it started does not, a new one does", () => {
		const demo = startDemo(chars(4), T0, [knock]);
		if (!demo) throw new Error("no demo");
		expect(interrupts(demo, [knock])).toBe(false);
		expect(
			interrupts(demo, [{ ...knock, since: new Date(T0 + 1).toISOString() }]),
		).toBe(true);
		expect(interrupts(demo, [knock, { ...knock, id: "real-3" }])).toBe(true);
	});
});

describe("store", () => {
	afterEach(() => vi.useRealTimers());
	const animals = { mode: "animals" } as Settings;

	function running() {
		const store = createStripStore();
		store.getState().applySettings(animals);
		store.getState().applySessions({ rev: 1, sessions: [SAMPLES.working] });
		return store;
	}

	test("starts, keeps the real sessions as they were, and ends on its own", () => {
		vi.useFakeTimers();
		const store = running();
		const sessions = store.getState().sessions;
		store.getState().startDemo(chars(5), Date.now(), seeded(4));
		const demo = store.getState().demo;
		expect(demo).not.toBeNull();
		expect(store.getState().sessions).toBe(sessions);
		vi.advanceTimersByTime(INTRO_MS + 5 * STATE_MS - 1);
		expect(store.getState().demo).toBe(demo);
		vi.advanceTimersByTime(1);
		expect(store.getState().demo).toBeNull();
		expect(store.getState().sessions).toBe(sessions);
	});

	test("the safety stop ends it at once", () => {
		vi.useFakeTimers();
		const store = running();
		store.getState().startDemo(chars(5));
		store.getState().applySessions({
			rev: 2,
			sessions: [SAMPLES.working, SAMPLES.permission],
		});
		expect(store.getState().demo).toBeNull();
	});

	test("a calm update does not stop it", () => {
		const store = running();
		store.getState().startDemo(chars(5));
		store.getState().applySessions({ rev: 2, sessions: [SAMPLES.ready] });
		expect(store.getState().demo).not.toBeNull();
		store.getState().stopDemo();
	});

	test("intro-demo again mid-demo restarts it", () => {
		vi.useFakeTimers();
		const store = running();
		store.getState().startDemo(chars(5), Date.now());
		const first = store.getState().demo;
		vi.advanceTimersByTime(10_000);
		store.getState().startDemo(chars(5), Date.now());
		const second = store.getState().demo;
		expect(second).not.toBe(first);
		expect(second?.startedAt).toBe((first?.startedAt ?? 0) + 10_000);
		// The first one's end no longer applies.
		vi.advanceTimersByTime(INTRO_MS + 5 * STATE_MS - 10_000);
		expect(store.getState().demo).toBe(second);
		vi.advanceTimersByTime(10_000);
		expect(store.getState().demo).toBeNull();
	});

	test("not in the plain strip, and switching to it ends the demo", () => {
		const plain = createStripStore();
		plain.getState().applySettings({ mode: "plain" } as Settings);
		plain.getState().startDemo(chars(5));
		expect(plain.getState().demo).toBeNull();
		const store = running();
		store.getState().startDemo(chars(5));
		store.getState().applySettings({ mode: "plain" } as Settings);
		expect(store.getState().demo).toBeNull();
	});
});

describe("the strip shows every demo character", () => {
	test("step gives all of them a slot: no den", () => {
		const demo = startDemo(chars(10), T0, []);
		if (!demo) throw new Error("no demo");
		const f = demoAt(demo, T0);
		const { model } = step(
			f.sessions,
			EMPTY_MEMORY,
			false,
			T0,
			() => CLIPS,
			false,
			Number.POSITIVE_INFINITY,
		);
		expect(model.characters).toHaveLength(10);
		expect(model.hidden).toEqual([]);
	});

	test("up to 6: the usual slots", () => {
		expect(introMetrics("medium", 4)).toEqual(metrics("medium"));
		expect(introMetrics("medium", 6)).toEqual(metrics("medium"));
	});

	test("more shrink to fit the 6-slot width, then wrap into rows", () => {
		for (const size of ["small", "medium", "large"] as const) {
			const max = MAX_SLOTS * metrics(size).s;
			for (let n = 7; n <= 40; n++) {
				const m = introMetrics(size, n);
				expect(m.cols * m.s).toBeLessThanOrEqual(max);
				expect(m.cols * m.rows).toBeGreaterThanOrEqual(n);
				expect(m.s).toBeGreaterThanOrEqual(Math.floor(metrics(size).s * 0.6));
				for (let i = 0; i < n; i++) {
					expect(slotX(i, m)).toBeGreaterThanOrEqual(0);
					expect(slotX(i, m) + m.s).toBeLessThanOrEqual(max);
				}
			}
		}
		const ten = introMetrics("medium", 10);
		expect(ten.rows).toBe(1);
		expect(ten.cols).toBe(10);
		expect(introMetrics("small", 20).rows).toBeGreaterThan(1);
	});

	test("rows: the first one is nearest the taskbar, and the next goes above it", () => {
		const m = introMetrics("small", 20);
		expect(m.rows).toBe(2);
		expect(slotTop(0, m, 0)).toBe(rowHeight(m) + m.headroom);
		expect(slotTop(0, m, m.cols)).toBe(m.headroom);
		expect(slotX(m.cols, m)).toBe(slotX(0, m));
		expect(characterRect(m.cols, false, 10, m).y).toBe(10 + m.headroom);
		// One row: as before.
		const one = metrics("medium");
		expect(slotTop(0, one, 5)).toBe(one.headroom);
	});
});
