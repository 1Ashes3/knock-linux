import { afterEach, expect, test, vi } from "vitest";

afterEach(() => {
	globalThis.__TAURI__ = undefined;
	vi.resetModules();
	vi.restoreAllMocks();
});

test("without Tauri it logs once and does not throw", async () => {
	const { sendHitRegions } = await import("./hitRegions");
	const log = vi.spyOn(console, "info").mockImplementation(() => {});
	sendHitRegions([]);
	sendHitRegions([]);
	expect(log).toHaveBeenCalledTimes(1);
});

test("with Tauri it invokes set_hit_regions with the regions", async () => {
	const { sendHitRegions } = await import("./hitRegions");
	const invoke = vi.fn().mockResolvedValue(undefined);
	globalThis.__TAURI__ = { core: { invoke } };
	const regions = [{ x: 136, y: 8, width: 48, height: 48 }];
	sendHitRegions(regions);
	expect(invoke).toHaveBeenCalledWith("set_hit_regions", { regions });
});
