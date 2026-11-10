import { describe, expect, it } from "vitest";
import type { SessionView } from "./bridge";
import {
	knocking,
	nextChange,
	READY_RISE_MS,
	rises,
	visibleSessions,
} from "./placement";

function session(id: string, needs: SessionView["needs"]): SessionView {
	return {
		id,
		project: id,
		tier: needs ? "needs_you" : "working",
		state: needs ? "needs_input" : "working",
		since: "2026-11-09T00:00:00Z",
		activity: null,
		needs,
		precision: "none",
		animal: "fox",
		agent: "claude-code",
		simulated: true,
	};
}

const busy = session("busy", null);
const asks = session("asks", { kind: "answer", label: null, seen: false });
const seen = session("seen", { kind: "permission", label: "Bash", seen: true });

describe("placement", () => {
	it("a knock counts only until someone has seen it", () => {
		expect([busy, asks, seen].map(knocking)).toEqual([false, true, false]);
	});

	const now = Date.parse("2026-11-09T00:00:00Z");

	it("desktop shows only the knocking sessions while one knocks", () => {
		expect(visibleSessions([busy, asks, seen], "desktop", now)).toEqual([asks]);
		expect(visibleSessions([busy, seen], "desktop", now)).toEqual([busy, seen]);
	});

	it("a finished session rises for two minutes", () => {
		const done = { ...busy, id: "done", state: "ready" as const };
		expect(rises(done, now + 1000)).toBe(true);
		expect(rises(done, now + READY_RISE_MS)).toBe(false);
		expect(visibleSessions([busy, done], "desktop", now + 1000)).toEqual([
			done,
		]);
		expect(nextChange([busy, done], now + 1000)).toBe(now + READY_RISE_MS);
		expect(nextChange([busy], now)).toBe(Number.POSITIVE_INFINITY);
	});

	it("top, hidden and no settings yet show everything", () => {
		for (const p of ["top", "hidden", undefined] as const)
			expect(visibleSessions([busy, asks], p, now)).toEqual([busy, asks]);
	});
});
