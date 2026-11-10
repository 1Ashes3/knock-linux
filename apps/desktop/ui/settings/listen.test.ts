import { afterEach, describe, expect, it, vi } from "vitest";
import { listen, listening } from "../overlay/bridge";

/** A fake Tauri `listen` whose promises resolve only when `settle()` is called. */
function fakeTauri() {
	const offs: ReturnType<typeof vi.fn>[] = [];
	const pending: (() => void)[] = [];
	const on = vi.fn(
		() =>
			new Promise<() => void>((resolve) => {
				const off = vi.fn();
				offs.push(off);
				pending.push(() => resolve(off));
			}),
	);
	(globalThis as { __TAURI__?: unknown }).__TAURI__ = {
		core: { invoke: vi.fn() },
		event: { listen: on },
	};
	return {
		offs,
		settle: async () => {
			for (const p of pending.splice(0)) p();
			await new Promise((r) => setTimeout(r, 0));
		},
	};
}

afterEach(() => {
	(globalThis as { __TAURI__?: unknown }).__TAURI__ = undefined;
});

describe("listening", () => {
	it("removes every listener when the effect is cleaned up", async () => {
		const t = fakeTauri();
		const stop = listening(
			listen("a", () => {}),
			listen("b", () => {}),
		);
		await t.settle();
		expect(t.offs.every((o) => o.mock.calls.length === 0)).toBe(true);
		stop();
		expect(t.offs.map((o) => o.mock.calls.length)).toEqual([1, 1]);
	});

	it("removes a listener that resolves after the cleanup ran", async () => {
		const t = fakeTauri();
		const stop = listening(listen("a", () => {}));
		stop();
		await t.settle();
		expect(t.offs[0]).toHaveBeenCalledTimes(1);
	});

	it("does not remove a listener twice", async () => {
		const t = fakeTauri();
		const stop = listening(listen("a", () => {}));
		await t.settle();
		stop();
		stop();
		expect(t.offs[0]).toHaveBeenCalledTimes(1);
	});

	it("works without Tauri", async () => {
		const stop = listening(listen("a", () => {}));
		await new Promise((r) => setTimeout(r, 0));
		expect(() => stop()).not.toThrow();
	});
});
