import { afterEach, expect, test, vi } from "vitest";
import { firstSession, type SessionView, sessionLabel } from "./sessions";

afterEach(() => {
	globalThis.__TAURI__ = undefined;
	vi.resetModules();
	vi.restoreAllMocks();
});

const a: SessionView = { id: "a", project: "knock", precision: "app_window" };
const b: SessionView = { id: "b", project: "other", precision: "none" };

test("first session is the most recent one and labels it", () => {
	expect(firstSession([a, b])).toBe(a);
	expect(sessionLabel(firstSession([a, b]))).toBe("knock · app_window");
});

test("no sessions", () => {
	expect(firstSession([])).toBeUndefined();
	expect(sessionLabel(firstSession([]))).toBe("no sessions");
});

test("a failing invoke logs once and resolves to no sessions", async () => {
	const { getSessions } = await import("./sessions");
	const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
	globalThis.__TAURI__ = {
		core: { invoke: vi.fn().mockRejectedValue(new Error("boom")) },
	};
	expect(await getSessions()).toEqual([]);
	expect(await getSessions()).toEqual([]);
	expect(warn).toHaveBeenCalledTimes(1);
});
