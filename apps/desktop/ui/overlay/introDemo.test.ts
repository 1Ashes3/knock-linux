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
	DEMO_MS,
	DEMO_STATES,
	DEMO_TOOL,
	type DemoCharacter,
	demoAt,
	demoDuration,
	demoEnd,
	INTRO_MS,
	interrupts,
	PICKED,
	pick,
	roundMs,
	startDemo,
	stateOf,
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
/** With the core's 18 s: 3 s of introduction, then 5 rounds of 3 s. */
const STATE_MS = 3000;

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

	test("each round: every picked character in a different state", () => {
		expect(roundMs(demo)).toBe(STATE_MS);
		for (let i = 0; i < DEMO_STATES.length; i++) {
			const at = T0 + INTRO_MS + i * STATE_MS;
			for (const t of [at, at + STATE_MS - 1]) {
				const f = demoAt(demo, t);
				expect(f.phase).toBe("states");
				expect(f.round).toBe(i);
				expect(f.sessions.map((s) => s.animal)).toEqual(
					demo.picked.map((c) => c.id),
				);
				expect(f.sessions).toHaveLength(PICKED);
				expect(new Set(f.sessions.map((s) => s.state)).size).toBe(PICKED);
				for (const s of f.sessions)
					expect(DEMO_STATES as readonly string[]).toContain(s.state);
				expect(f.nextAt).toBe(at + STATE_MS);
				for (const s of f.sessions) expect(Date.parse(s.since)).toBe(at);
			}
		}
	});

	test("over the 5 rounds each character goes through all 5 states", () => {
		const visited = new Map<string, string[]>();
		for (let i = 0; i < DEMO_STATES.length; i++)
			for (const s of demoAt(demo, T0 + INTRO_MS + i * STATE_MS).sessions)
				visited.set(s.animal ?? "", [
					...(visited.get(s.animal ?? "") ?? []),
					s.state,
				]);
		expect(visited.size).toBe(PICKED);
		for (const states of visited.values())
			expect([...states].sort()).toEqual([...DEMO_STATES].sort());
	});

	test("the rotation: sleep, work, knock, failed, done, in that order", () => {
		expect([0, 1, 2, 3, 4].map((i) => stateOf(0, i))).toEqual([
			"idle",
			"working",
			"needs_input",
			"failed",
			"ready",
		]);
		for (let i = 0; i < 5; i++)
			expect(new Set([0, 1, 2, 3, 4].map((k) => stateOf(k, i))).size).toBe(5);
	});

	test("each state has the tier and the chip text the real one has", () => {
		const in_ = (state: string) =>
			demoAt(demo, T0 + INTRO_MS)
				.sessions.concat(demoAt(demo, T0 + INTRO_MS + STATE_MS).sessions)
				.find((s) => s.state === state) as SessionView;
		expect(in_("idle").tier).toBe("resting");
		expect(in_("idle").needs).toBeNull();
		expect(in_("working").tier).toBe("working");
		expect(in_("needs_input").tier).toBe("needs_you");
		expect(in_("needs_input").needs).toEqual({
			kind: "permission",
			label: DEMO_TOOL,
			seen: false,
		});
		expect(stateLabel(in_("needs_input"), 0, 10)).toBe(
			`Needs permission: ${DEMO_TOOL}`,
		);
		expect(in_("failed").tier).toBe("needs_you");
		expect(in_("failed").needs).toBeNull();
		expect(stateLabel(in_("failed"), 0, 10)).toBe("Stopped with an error");
		expect(in_("ready").tier).toBe("ready");
		expect(stateLabel(in_("ready"), 0, 10)).toBe("Done, ready to review");
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
		expect(end).toBe(T0 + DEMO_MS);
		expect(end).toBe(T0 + INTRO_MS + 5 * STATE_MS);
		expect(demoAt(demo, end - 1).phase).toBe("states");
		const f = demoAt(demo, end);
		expect(f.phase).toBe("over");
		expect(f.sessions).toEqual([]);
		expect(f.nextAt).toBe(Number.POSITIVE_INFINITY);
	});

	test("fewer than 4: each still in a different state, and visits all 5", () => {
		for (const n of [1, 2, 3]) {
			const d = startDemo(chars(n), T0, [], seeded(2));
			if (!d) throw new Error("no demo");
			const visited = new Map<string, Set<string>>();
			for (let i = 0; i < DEMO_STATES.length; i++) {
				const f = demoAt(d, T0 + INTRO_MS + i * STATE_MS);
				expect(f.sessions).toHaveLength(n);
				expect(new Set(f.sessions.map((s) => s.state)).size).toBe(n);
				for (const s of f.sessions)
					visited.set(
						s.animal ?? "",
						(visited.get(s.animal ?? "") ?? new Set()).add(s.state),
					);
			}
			for (const states of visited.values()) expect(states.size).toBe(5);
		}
	});
});

describe("duration from the event", () => {
	test("durationMs from the payload; 18 000 when missing or unusable", () => {
		expect(demoDuration({ durationMs: 18_000 })).toBe(18_000);
		expect(demoDuration({ durationMs: 28_000 })).toBe(28_000);
		for (const p of [
			undefined,
			null,
			{},
			{ durationMs: "18000" },
			{ durationMs: Number.NaN },
			{ durationMs: -1 },
			{ durationMs: INTRO_MS },
		])
			expect(demoDuration(p)).toBe(DEMO_MS);
		expect(DEMO_MS).toBe(18_000);
	});

	test("intro stays 3 s; the rest splits evenly into 5 rounds", () => {
		const d = startDemo(chars(6), T0, [], seeded(5), 28_000);
		if (!d) throw new Error("no demo");
		expect(roundMs(d)).toBe(5000);
		expect(demoAt(d, T0 + INTRO_MS - 1).phase).toBe("intro");
		for (let i = 0; i < 5; i++) {
			const at = T0 + INTRO_MS + i * 5000;
			expect(demoAt(d, at).round).toBe(i);
			expect(demoAt(d, at + 4999).round).toBe(i);
			expect(demoAt(d, at).nextAt).toBe(at + 5000);
		}
		expect(demoEnd(d)).toBe(T0 + 28_000);
		expect(demoAt(d, T0 + 28_000 - 1).phase).toBe("states");
		expect(demoAt(d, T0 + 28_000).phase).toBe("over");
	});

	test("an uneven split still ends exactly at durationMs", () => {
		const d = startDemo(chars(4), T0, [], seeded(5), 10_001);
		if (!d) throw new Error("no demo");
		expect(demoAt(d, T0 + 10_000).phase).toBe("states");
		expect(demoAt(d, T0 + 10_000).nextAt).toBe(T0 + 10_001);
		expect(demoAt(d, T0 + 10_001).phase).toBe("over");
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
		store.getState().startDemo(chars(5), 18_000, Date.now(), seeded(4));
		const demo = store.getState().demo;
		expect(demo).not.toBeNull();
		expect(store.getState().sessions).toBe(sessions);
		vi.advanceTimersByTime(INTRO_MS + 5 * STATE_MS - 1);
		expect(store.getState().demo).toBe(demo);
		vi.advanceTimersByTime(1);
		expect(store.getState().demo).toBeNull();
		expect(store.getState().sessions).toBe(sessions);
	});

	test("lasts the durationMs it was given", () => {
		vi.useFakeTimers();
		const store = running();
		store.getState().startDemo(chars(5), 28_000, Date.now(), seeded(4));
		vi.advanceTimersByTime(27_999);
		expect(store.getState().demo).not.toBeNull();
		vi.advanceTimersByTime(1);
		expect(store.getState().demo).toBeNull();
	});

	test("without a duration: 18 s", () => {
		vi.useFakeTimers();
		const store = running();
		store.getState().startDemo(chars(5));
		vi.advanceTimersByTime(17_999);
		expect(store.getState().demo).not.toBeNull();
		vi.advanceTimersByTime(1);
		expect(store.getState().demo).toBeNull();
	});

	test("no start while a real session is asking", () => {
		const store = running();
		store.getState().applySessions({
			rev: 2,
			sessions: [SAMPLES.working, SAMPLES.permission],
		});
		store.getState().startDemo(chars(5));
		expect(store.getState().demo).toBeNull();
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
		store.getState().startDemo(chars(5), 18_000, Date.now());
		const first = store.getState().demo;
		vi.advanceTimersByTime(10_000);
		store.getState().startDemo(chars(5), 18_000, Date.now());
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
