export interface HitRegion {
	x: number;
	y: number;
	width: number;
	height: number;
}

let warned = false;

/** Sends `set_hit_regions` (§8.5). Without Tauri (plain browser) logs once and skips. */
export function sendHitRegions(regions: HitRegion[]): void {
	const invoke = globalThis.__TAURI__?.core.invoke;
	if (!invoke) {
		if (!warned) console.info("Tauri not available; skipping set_hit_regions.");
		warned = true;
		return;
	}
	invoke("set_hit_regions", { regions }).catch((e: unknown) =>
		console.error("set_hit_regions failed", e),
	);
}
