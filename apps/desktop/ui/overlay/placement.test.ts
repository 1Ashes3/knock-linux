import { describe, expect, it } from "vitest";
import type { SessionView } from "./bridge";
import { knocking, visibleSessions } from "./placement";

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

	it("desktop shows only the knocking sessions while one knocks", () => {
		expect(visibleSessions([busy, asks, seen], "desktop")).toEqual([asks]);
		expect(visibleSessions([busy, seen], "desktop")).toEqual([busy, seen]);
	});

	it("top, hidden and no settings yet show everything", () => {
		for (const p of ["top", "hidden", undefined] as const)
			expect(visibleSessions([busy, asks], p)).toEqual([busy, asks]);
	});
});
