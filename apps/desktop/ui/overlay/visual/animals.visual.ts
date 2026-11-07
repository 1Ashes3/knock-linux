import { expect, type Page, test } from "@playwright/test";
import type { AwaySummary, SessionView, Settings } from "../bridge";
import { SAMPLE_KEYS, type SampleKey } from "../dev/samples";

// Animals mode (§12.1): every state x taskbar x animation effects, the three sizes, a
// strip with more than 5 sessions, and the behaviour around them. Time is Playwright's
// fake clock, paused while the page loads, so every clip is at a known point.

const T0 = Date.parse("2026-11-01T12:00:00Z");
const TASKBARS = ["light", "dark"] as const;
const MOTION = { on: "no-preference", off: "reduce" } as const;
/** Long enough for every calm animal to settle (ADR 0031). */
const SETTLE_MS = 6000;

interface Mock {
	set(id: string, patch: Partial<SessionView>): void;
	remove(id: string): void;
	settings(patch: Partial<Settings>): void;
	away(summary: AwaySummary | number): void;
}

async function open(
	page: Page,
	query: string,
	motion: keyof typeof MOTION = "on",
	drawn?: number,
) {
	await page.emulateMedia({ reducedMotion: MOTION[motion] });
	await page.clock.install({ time: T0 });
	await page.clock.pauseAt(T0 + 1000);
	await page.goto(`/overlay/dev/mock.html?mode=animals&${query}`);
	const characters = page.locator(".animals .character");
	await expect(characters.first()).toBeVisible();
	const n = drawn ?? (await characters.count());
	await expect(page.locator(".stage")).toHaveAttribute("data-drawn", String(n));
}

const mock = (page: Page, run: (m: Mock) => void) =>
	page.evaluate(
		`(${run.toString()})(globalThis.__knockMock)` as unknown as () => void,
	);

/** The box around everything `selector` matches. */
async function union(page: Page, selector: string) {
	const boxes = await page.locator(selector).evaluateAll((els) =>
		els.map((e) => {
			const r = e.getBoundingClientRect();
			return { x: r.x, y: r.y, right: r.right, bottom: r.bottom };
		}),
	);
	if (!boxes.length) throw new Error(`nothing matches ${selector}`);
	const x = Math.min(...boxes.map((b) => b.x));
	const y = Math.min(...boxes.map((b) => b.y));
	return {
		x,
		y,
		width: Math.max(...boxes.map((b) => b.right)) - x,
		height: Math.max(...boxes.map((b) => b.bottom)) - y,
	};
}

/** Everything `selector` matches lies inside the mock's window (dev/mock.ts `placeWindow`). */
async function expectInWindow(page: Page, selector: string) {
	const win = JSON.parse(
		(await page.locator("#root").getAttribute("data-window")) ?? "{}",
	);
	const box = await union(page, selector);
	expect(box.x).toBeGreaterThanOrEqual(win.x);
	expect(box.y).toBeGreaterThanOrEqual(win.y);
	expect(box.x + box.width).toBeLessThanOrEqual(win.x + win.width);
	expect(box.y + box.height).toBeLessThanOrEqual(win.y + win.height);
}

/** The strip (and the away card, if it shows) plus a margin and the fake taskbar under it. */
async function shot(page: Page, name: string) {
	// Measure the card where it rests, not part-way through its rise.
	for (const card of await page.locator(".away").all())
		await card.evaluate((e) => {
			for (const a of e.getAnimations()) a.finish();
		});
	const box = await union(page, ".animals, .away");
	await expect(page).toHaveScreenshot(`${name}.png`, {
		clip: {
			x: box.x - 8,
			y: box.y - 8,
			width: box.width + 16,
			height: box.height + 8 + 48,
		},
	});
}

const loop = (page: Page) => page.locator(".stage");

const CHIP: Record<SampleKey, string | null> = {
	permission: "Needs permission: PowerShell",
	permission_no_tool: "Needs permission",
	question: "Has a question",
	seen: "Waiting for you",
	failed: "Stopped with an error",
	stalled: "No progress for 13 min",
	ready: "Done, ready to review",
	working: null,
	idle: null,
	ended: "Session ended",
};
const LOOPS: SampleKey[] = [
	"permission",
	"permission_no_tool",
	"question",
	"failed",
	"stalled",
];
const RAISED: SampleKey[] = ["permission", "permission_no_tool", "question"];

for (const motion of ["on", "off"] as const)
	for (const taskbar of TASKBARS)
		for (const key of SAMPLE_KEYS)
			test(`animals: ${key}, ${taskbar} taskbar, animation ${motion}`, async ({
				page,
			}) => {
				await open(page, `show=${key}&taskbar=${taskbar}`, motion);
				await page.clock.runFor(SETTLE_MS);
				// §6 CPU: only Needs-you clips keep the loop going, never with effects off.
				await expect(loop(page)).toHaveAttribute(
					"data-loop",
					motion === "on" && LOOPS.includes(key) ? "running" : "stopped",
				);
				const chip = page.locator(".chip");
				if (CHIP[key]) await expect(chip).toHaveText(CHIP[key]);
				else await expect(chip).toHaveCount(0);
				// Effects off: a badge instead of motion; Failed always has the red marker.
				await expect(page.locator(".badge")).toHaveCount(
					motion === "off" && key !== "failed" ? 1 : 0,
				);
				await expect(page.locator(".marker")).toHaveCount(
					key === "failed" ? 1 : 0,
				);
				// Stepped up only while knocking with effects on.
				const raised = motion === "on" && RAISED.includes(key);
				const top = await page
					.locator(".character")
					.evaluate((e) => (e as HTMLElement).style.top);
				const stage = await page
					.locator(".stage")
					.evaluate((e) => Number.parseFloat((e as HTMLElement).style.top));
				expect(Number.parseFloat(top)).toBe(stage + 16 - (raised ? 12 : 0));
				await shot(page, `animals-${key}-${taskbar}-motion-${motion}`);
			});

// Every size with all samples: 5 characters and the den (FR-10).
for (const size of ["small", "medium", "large"])
	for (const taskbar of TASKBARS)
		test(`animals: all samples, ${size}, ${taskbar} taskbar`, async ({
			page,
		}) => {
			await open(page, `size=${size}&taskbar=${taskbar}`);
			await page.clock.runFor(SETTLE_MS);
			await expect(
				page.getByRole("img", {
					name: "5 more sessions: 1 needs you, 1 ready, 1 working, 2 resting",
				}),
			).toBeVisible();
			await expect(page.locator(".animals .den-count")).toHaveText([
				"1",
				"1",
				"1",
				"2",
			]);
			await expect(page.locator(".animals .character")).toHaveCount(5);
			await shot(page, `animals-all-${size}-${taskbar}`);
		});

// `knock sim eight-den` (§12.3, Phase 4 exit) in animals mode: three knock or alert, two
// present, and the den holds 1 ready and 2 working.
const EIGHT_DEN_NAME = "3 more sessions: 1 ready, 2 working";
const EIGHT_DEN_CHIPS = ["s21", "s22", "s23", "s24", "s27"];

for (const motion of ["on", "off"] as const)
	for (const taskbar of TASKBARS)
		test(`animals: den, eight-den, ${taskbar} taskbar, animation ${motion}`, async ({
			page,
		}) => {
			await open(page, `set=eight-den&taskbar=${taskbar}`, motion);
			await page.clock.runFor(SETTLE_MS);
			await expect(page.locator(".animals .character")).toHaveCount(5);
			await expect(
				page.getByRole("img", { name: EIGHT_DEN_NAME }),
			).toBeVisible();
			await expect(page.locator(".chip")).toHaveCount(EIGHT_DEN_CHIPS.length);
			// The den is drawn by the UI and never catches the mouse.
			await expect(page.locator(".den")).toHaveCSS("pointer-events", "none");
			await expect(loop(page)).toHaveAttribute(
				"data-loop",
				motion === "on" ? "running" : "stopped",
			);
			await shot(page, `animals-den-eight-${taskbar}-motion-${motion}`);
		});

for (const taskbar of TASKBARS)
	test(`animals: den, eight-den, small, ${taskbar} taskbar`, async ({
		page,
	}) => {
		await open(page, `set=eight-den&size=small&taskbar=${taskbar}`);
		await page.clock.runFor(SETTLE_MS);
		await expect(page.getByRole("img", { name: EIGHT_DEN_NAME })).toBeVisible();
		await shot(page, `animals-den-eight-small-${taskbar}`);
	});

test("animals: a den session that needs you steps out and knocks; they swap back after", async ({
	page,
}) => {
	await open(page, "set=eight-den&taskbar=dark");
	await page.clock.runFor(SETTLE_MS);
	const infra = () =>
		page.getByRole("button", { name: /^infra,/ }).evaluate((e) => ({
			left: Number.parseFloat((e as HTMLElement).style.left),
			top: Number.parseFloat((e as HTMLElement).style.top),
		}));
	const stage = await page
		.locator(".stage")
		.evaluate((e) => Number.parseFloat((e as HTMLElement).style.top));
	await mock(page, (m) =>
		m.set("s26", {
			state: "needs_input",
			tier: "needs_you",
			needs: { kind: "permission", label: "Bash", seen: false },
		}),
	);
	await expect(
		page.getByRole("button", { name: "infra, Needs permission: Bash" }),
	).toBeVisible();
	// api-gateway, the last calm character, went into the den in its place.
	await expect(page.getByRole("button", { name: /^api-gateway/ })).toHaveCount(
		0,
	);
	await expect(
		page.getByRole("img", { name: "3 more sessions: 2 ready, 1 working" }),
	).toBeVisible();
	// It steps up (its hit region rises) and knocks.
	await expect.poll(async () => (await infra()).top).toBe(stage + 16 - 12);
	await page.clock.runFor(2000);
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await shot(page, "animals-den-step-out");
	await mock(page, (m) =>
		m.set("s26", { state: "working", tier: "working", needs: null }),
	);
	await expect(page.getByRole("button", { name: /^infra/ })).toHaveCount(0);
	await expect(
		page.getByRole("button", { name: "api-gateway, Done, ready to review" }),
	).toBeVisible();
	await expect(page.getByRole("img", { name: EIGHT_DEN_NAME })).toBeVisible();
});

test("animals: with effects off a den session that needs you swaps in at once, still", async ({
	page,
}) => {
	await open(page, "set=eight-den", "off");
	await mock(page, (m) =>
		m.set("s28", {
			state: "needs_input",
			tier: "needs_you",
			needs: { kind: "answer", label: null, seen: false },
		}),
	);
	const search = page.getByRole("button", {
		name: "search, Has a question",
	});
	await expect(search).toBeVisible();
	await expect(search.locator(".badge")).toHaveCount(1);
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
});

// The "While you were away" card (FR-9), above everything so it never covers a chip.
for (const taskbar of TASKBARS)
	test(`animals: away card above knocking characters, ${taskbar} taskbar`, async ({
		page,
	}) => {
		await open(page, `set=eight-den&taskbar=${taskbar}&away=7`);
		await page.clock.runFor(SETTLE_MS);
		const card = page.locator(".away");
		await expect(card).toBeVisible();
		await expect(card.locator("li")).toHaveCount(5);
		await expect(card.locator(".away-more")).toHaveText("and 2 more");
		// Nothing overlaps: the card is above every chip, leader, character, den and button,
		// and fully inside the window the core gives (6 slots wide).
		await card.evaluate((e) => {
			for (const a of e.getAnimations()) a.finish();
		});
		const box = await card.boundingBox();
		const others = await union(
			page,
			".chip, .leaders, .character, .den, .dismiss, .stage",
		);
		if (!box) throw new Error("no card");
		expect(box.y + box.height).toBeLessThanOrEqual(others.y);
		await expectInWindow(page, ".away, .animals");
		await shot(page, `animals-away-${taskbar}`);
	});

test("animals: away card, small, light taskbar", async ({ page }) => {
	await open(page, "set=eight-den&size=small&taskbar=light&away=3");
	await page.clock.runFor(SETTLE_MS);
	await expect(page.locator(".away li")).toHaveCount(3);
	await shot(page, "animals-away-small-light");
});

test("animals: away card with effects off just shows", async ({ page }) => {
	await open(page, "show=permission,ready&taskbar=dark&away=2", "off");
	await expect(page.locator(".away")).toHaveCSS("animation-name", "none");
	await shot(page, "animals-away-dark-motion-off");
});

test("animals: away card is a hit region while it shows, and folds after 60 s or on click", async ({
	page,
}) => {
	await open(page, "show=ready,failed");
	const log = page.locator("#log");
	const last = async (cmd: string) =>
		((await log.textContent()) ?? "")
			.split("\n")
			.find((l) => l.startsWith(`${cmd} `));
	const regions = async () =>
		JSON.parse((await last("set_hit_regions"))?.slice(16) ?? "{}").regions;
	await mock(page, (m) => m.away(2));
	// The window stays 384 wide and grows up by the card (77) plus the gap (8).
	await expect
		.poll(() => last("request_strip_size"))
		.toBe('request_strip_size {"width":384,"height":241}');
	const strip = await page.locator(".animals").boundingBox();
	await expect.poll(async () => (await regions()).length).toBe(4);
	expect((await regions())[3]).toEqual({
		x: (strip?.x ?? 0) + 384 - 264,
		y: (strip?.y ?? 0) - 8 - 77,
		width: 264,
		height: 77,
	});
	await page.clock.runFor(59_000);
	await expect(page.locator(".away")).toBeVisible();
	await page.clock.runFor(1_100);
	await expect(page.locator(".away")).toHaveCount(0);
	await expect.poll(async () => (await regions()).length).toBe(3);
	await expect
		.poll(() => last("request_strip_size"))
		.toBe('request_strip_size {"width":384,"height":156}');
	// A new one, closed with a click: dismiss_away_summary once per card.
	await mock(page, (m) => m.away(1));
	await page.locator(".away").click();
	await expect(page.locator(".away")).toHaveCount(0);
	expect(
		(await log.textContent())?.match(/dismiss_away_summary/g),
	).toHaveLength(2);
	// Mode switch keeps a showing card.
	await mock(page, (m) => m.away(1));
	await mock(page, (m) => m.settings({ mode: "plain" }));
	await expect(page.locator(".away")).toBeVisible();
});

test("animals: a mixed strip, dark taskbar", async ({ page }) => {
	await open(page, "show=permission,ready,failed,working,idle");
	await page.clock.runFor(SETTLE_MS);
	await shot(page, "animals-mixed-dark");
});

for (const scale of [1.5, 2])
	test.describe(`at ${scale * 100}% scaling`, () => {
		test.use({ deviceScaleFactor: scale });
		test(`animals: crisp at ${scale * 100}%`, async ({ page }) => {
			await open(page, "show=permission,ready,idle&taskbar=light");
			await page.clock.runFor(SETTLE_MS);
			const canvas = page.locator(".stage canvas");
			// The canvas has a device pixel for every screen pixel.
			const { w, cw } = await canvas.evaluate((c) => ({
				w: (c as HTMLCanvasElement).width,
				cw: c.getBoundingClientRect().width,
			}));
			expect(w).toBe(cw * scale);
			await shot(page, `animals-scale-${scale * 100}`);
		});
	});

test("hover shows the tooltip: project, state, time in state, decision type", async ({
	page,
}) => {
	await open(page, "show=working,seen,ready&taskbar=dark");
	await page.clock.runFor(SETTLE_MS);
	await page
		.getByRole("button", { name: "notes, Waiting for you", exact: true })
		.hover();
	const tip = page.locator(".tooltip");
	await expect(tip).toBeVisible();
	await expect(tip.locator("span")).toHaveText([
		"notes",
		"Waiting for you",
		"For 3 min",
		"Decision: permission to use Bash",
	]);
	await shot(page, "animals-tooltip-dark");
	await page
		.getByRole("button", { name: "mobile-app, Working", exact: true })
		.hover();
	await expect(tip.locator("span")).toHaveText([
		"mobile-app",
		"Working",
		"For 3 min",
	]);
	await page.mouse.move(5, 5);
	await expect(tip).toHaveCount(0);
});

test("Narrator names every character, and nothing can take focus", async ({
	page,
}) => {
	await open(page, "show=permission,question,seen,failed,ready");
	for (const name of [
		"knock, Needs permission: PowerShell",
		"website, Has a question",
		"notes, Waiting for you",
		"billing-service, Stopped with an error",
		"docs, Done, ready to review",
		"Dismiss billing-service",
	])
		await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
	await expect(page.locator("canvas")).toHaveAttribute("aria-hidden", "true");
	await expect(
		page.locator(".animals :is([tabindex], button, input, a)"),
	).toHaveCount(0);
});

test("size requests, hit regions, jump and dismiss go to the core", async ({
	page,
}) => {
	await open(page, "show=ready,failed");
	const log = page.locator("#log");
	await expect(log).toContainText(
		'request_strip_size {"width":384,"height":156}',
	);
	// Two characters plus the failed one's dismiss button, relative to the strip
	// (the mock puts the strip 8 px from the right and 48 px up).
	const regions = async () => {
		const text = (await log.textContent()) ?? "";
		const line = text.split("\n").find((l) => l.startsWith("set_hit_regions"));
		return JSON.parse(line?.slice("set_hit_regions ".length) ?? "{}").regions;
	};
	const strip = await page.locator(".animals").boundingBox();
	const at = (x: number, y: number, w: number, h: number) => ({
		x: (strip?.x ?? 0) + x,
		y: (strip?.y ?? 0) + y,
		width: w,
		height: h,
	});
	await expect
		.poll(regions)
		.toEqual([
			at(320, 92, 64, 64),
			at(256, 92, 64, 64),
			at(256 + 45, 92, 19, 19),
		]);
	await page
		.getByRole("button", { name: "docs, Done, ready to review" })
		.click();
	await expect(log).toContainText('jump_to_session {"id":"ready"}');
	await expect(page.getByRole("button", { name: "docs, Idle" })).toBeVisible();
	await page.getByRole("button", { name: "Dismiss billing-service" }).click();
	await expect(log).toContainText('dismiss {"id":"failed"}');
	await expect(
		page.getByRole("button", { name: /billing-service/ }),
	).toHaveCount(0);
});

test("knocking raises the hit region; leaving Needs you steps down and lowers it", async ({
	page,
}) => {
	await open(page, "show=working");
	const character = page.locator(".character");
	const top = () =>
		character.evaluate((e) => Number.parseFloat((e as HTMLElement).style.top));
	const low = await top();
	await mock(page, (m) =>
		m.set("working", {
			state: "needs_input",
			tier: "needs_you",
			needs: { kind: "permission", label: "Bash", seen: false },
		}),
	);
	await expect.poll(top).toBe(low - 12);
	await page.clock.runFor(2000);
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await shot(page, "animals-knocking");
	await mock(page, (m) =>
		m.set("working", { state: "working", tier: "working", needs: null }),
	);
	await expect.poll(top).toBe(low);
	await page.clock.runFor(200); // half-way down
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await shot(page, "animals-stepping-down");
	await page.clock.runFor(2000);
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
});

test("the render loop stops once every animal has settled (ADR 0031)", async ({
	page,
}) => {
	await open(page, "show=working,idle,ready,seen,ended");
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await page.clock.runFor(SETTLE_MS);
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
	// The "z" stops moving too.
	await expect(page.locator(".knock-zzz")).toHaveClass("knock-zzz settled");
	// Nothing at all asks for a frame or a timer while everything is still (Pixi included).
	await page.evaluate(() => {
		const w = window as unknown as Record<string, unknown>;
		w.__asked = 0;
		for (const name of ["requestAnimationFrame", "setTimeout", "setInterval"]) {
			const original = w[name] as (...a: unknown[]) => unknown;
			w[name] = (...a: unknown[]) => {
				(w.__asked as number)++;
				return original(...a);
			};
		}
	});
	await page.clock.runFor(10_000);
	expect(
		await page.evaluate(() => (window as { __asked?: number }).__asked),
	).toBe(0);
	// A new state change plays once more, then stops again.
	await mock(page, (m) => m.set("idle", { state: "working", tier: "working" }));
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await page.clock.runFor(SETTLE_MS);
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
});

test("turning Animation effects off stops the loop and shows badges at once", async ({
	page,
}) => {
	await open(page, "show=permission,failed,working");
	await page.clock.runFor(1000);
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await page.emulateMedia({ reducedMotion: "reduce" });
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
	await expect(page.locator(".badge")).toHaveCount(2);
	await page.emulateMedia({ reducedMotion: "no-preference" });
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await expect(page.locator(".badge")).toHaveCount(0);
});

test("an ended session fades 60 s after it ended and loses its hit region", async ({
	page,
}) => {
	await open(page, "show=ended,idle");
	await page.clock.runFor(SETTLE_MS);
	const ended = page.getByRole("button", { name: "infra, Session ended" });
	await expect(ended).toBeVisible();
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
	// `since` is 20 s before the page loaded: the fade starts 40 s after load.
	await page.clock.runFor(40_000 - SETTLE_MS + 100);
	await expect(ended).toHaveCount(0);
	await expect(page.locator(".chip")).toHaveCount(0);
	await page.clock.runFor(1000);
	await expect(page.locator(".animals .character")).toHaveCount(1);
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
	await expect(page.locator(".stage")).toHaveAttribute("data-drawn", "1");
});

test("a session that leaves the list fades out", async ({ page }) => {
	await open(page, "show=ready,idle");
	await page.clock.runFor(SETTLE_MS);
	await mock(page, (m) => m.remove("ready"));
	await expect(page.getByRole("button", { name: /docs/ })).toHaveCount(0);
	await expect(loop(page)).toHaveAttribute("data-loop", "running");
	await expect(page.locator(".stage")).toHaveAttribute("data-drawn", "2");
	await page.clock.runFor(1000);
	await expect(page.locator(".stage")).toHaveAttribute("data-drawn", "1");
	await expect(loop(page)).toHaveAttribute("data-loop", "stopped");
});

test("mode and size switch live, without a restart (FR-3)", async ({
	page,
}) => {
	await page.goto("/overlay/dev/mock.html?show=permission,ready");
	const log = page.locator("#log");
	await expect(page.locator(".strip .pill")).toHaveCount(2);
	await mock(page, (m) => m.settings({ mode: "animals" }));
	await expect(page.locator(".animals .character")).toHaveCount(2);
	await expect(page.locator(".strip")).toHaveCount(0);
	await expect(log).toContainText(
		'request_strip_size {"width":384,"height":156}',
	);
	await mock(page, (m) => m.settings({ size: "large" }));
	await expect(log).toContainText(
		'request_strip_size {"width":480,"height":184}',
	);
	await mock(page, (m) => m.settings({ mode: "plain" }));
	await expect(page.locator(".strip .pill")).toHaveCount(2);
	await expect(page.locator(".animals")).toHaveCount(0);
	await expect(log).toContainText(
		'request_strip_size {"width":160,"height":80}',
	);
});

test("an animal with no art is drawn as the fox, and nothing breaks the CSP", async ({
	page,
}) => {
	const errors: string[] = [];
	page.on("console", (m) => {
		if (m.type() === "error" && !m.text().includes("404"))
			errors.push(m.text());
	});
	page.on("pageerror", (e) => errors.push(e.message));
	await open(page, "show=working,idle&animal=unicorn");
	await page.clock.runFor(SETTLE_MS);
	await shot(page, "animals-unknown-animal");
	expect(errors).toEqual([]);
});

test("the window never asks to be wider than 6 slots, in either mode, card or not (§9.2)", async ({
	page,
}) => {
	for (const mode of ["plain", "animals"])
		for (const [size, slot] of [
			["small", 48],
			["medium", 64],
			["large", 80],
		] as const) {
			await page.goto(`/overlay/dev/mock.html?mode=${mode}&size=${size}`);
			const log = page.locator("#log");
			await expect(log).toContainText("request_strip_size");
			await mock(page, (m) => m.away(7));
			await expect(page.locator(".away")).toBeVisible();
			await page.locator(".away").evaluate((e) => {
				for (const a of e.getAnimations()) a.finish();
			});
			await expectInWindow(page, ".away, .strip, .animals");
			await page.locator(".away").click();
			await expect(page.locator(".away")).toHaveCount(0);
			const widths = ((await log.textContent()) ?? "")
				.split("\n")
				.filter((l) => l.startsWith("request_strip_size "))
				.map((l) => JSON.parse(l.slice(19)).width as number);
			expect(widths.length, `${mode} ${size}`).toBeGreaterThanOrEqual(3);
			for (const w of widths)
				expect(w, `${mode} ${size}`).toBeLessThanOrEqual(6 * slot);
		}
});
