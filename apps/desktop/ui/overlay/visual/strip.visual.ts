import { expect, type Page, test } from "@playwright/test";
import type { AwaySummary } from "../bridge";
import { SAMPLE_KEYS } from "../dev/samples";

const TASKBARS = ["light", "dark"] as const;

interface Mock {
	set(id: string, patch: Record<string, unknown>): void;
	away(summary: AwaySummary | number): void;
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
const MOTION = { on: "no-preference", off: "reduce" } as const;

async function open(
	page: Page,
	query: string,
	motion: keyof typeof MOTION = "on",
) {
	await page.emulateMedia({ reducedMotion: MOTION[motion] });
	await page.goto(`/overlay/dev/mock.html?${query}`);
	await expect(page.locator(".strip .slot").first()).toBeVisible();
}

/** The strip (and the away card, if it shows) plus a margin and the fake taskbar under it. */
async function shot(page: Page, name: string) {
	// Measure the card where it rests, not part-way through its rise.
	for (const card of await page.locator(".away").all())
		await card.evaluate((e) => {
			for (const a of e.getAnimations()) a.finish();
		});
	const box = await union(page, ".strip, .away");
	await expect(page).toHaveScreenshot(`${name}.png`, {
		clip: {
			x: box.x - 8,
			y: box.y - 8,
			width: box.width + 16,
			height: box.height + 8 + 48,
		},
	});
}

// Every state x taskbar x animation effects, in plain mode (§12.1).
for (const motion of ["on", "off"] as const)
	for (const taskbar of TASKBARS)
		for (const key of SAMPLE_KEYS)
			test(`${key}, ${taskbar} taskbar, animation ${motion}`, async ({
				page,
			}) => {
				await open(page, `show=${key}&taskbar=${taskbar}`, motion);
				// The screenshot freezes animations, so check the knock itself here.
				const knocking = page.locator(".knock .glyph");
				if (
					key === "permission" ||
					key === "permission_no_tool" ||
					key === "question"
				)
					await expect(knocking).toHaveCSS(
						"animation-name",
						motion === "on" ? "knock" : "none",
					);
				else await expect(knocking).toHaveCount(0);
				await shot(page, `${key}-${taskbar}-motion-${motion}`);
			});

// Every size with all samples: 5 pills and the den (FR-10).
for (const size of ["small", "medium", "large"])
	for (const taskbar of TASKBARS)
		test(`all samples, ${size}, ${taskbar} taskbar`, async ({ page }) => {
			await open(page, `size=${size}&taskbar=${taskbar}`);
			await expect(page.locator(".strip .slot")).toHaveCount(6);
			await expect(
				page.getByRole("img", {
					name: "5 more sessions: 1 needs you, 1 ready, 1 working, 2 resting",
				}),
			).toBeVisible();
			await shot(page, `all-${size}-${taskbar}`);
		});

// `knock sim eight-den` (§12.3, Phase 4 exit): 3 Needs you and 2 Ready get pills; the den
// holds the rest.
const EIGHT_DEN_PILLS = [
	"web-shop, Done, ready to review",
	"api-gateway, Done, ready to review",
	"data-jobs, Needs permission: PowerShell",
	"mobile-app, Stopped with an error",
	"billing, Has a question",
];
const EIGHT_DEN_NAME = "3 more sessions: 1 ready, 2 working";

for (const motion of ["on", "off"] as const)
	for (const taskbar of TASKBARS)
		test(`den: eight-den, ${taskbar} taskbar, animation ${motion}`, async ({
			page,
		}) => {
			await open(page, `set=eight-den&taskbar=${taskbar}`, motion);
			await expect(page.locator(".strip .pill")).toHaveCount(5);
			for (const name of EIGHT_DEN_PILLS)
				await expect(
					page.getByRole("button", { name, exact: true }),
				).toBeVisible();
			const den = page.getByRole("img", { name: EIGHT_DEN_NAME });
			await expect(den).toBeVisible();
			// Counts carry the tier glyph, not just its color (§6).
			await expect(den.locator(".den-count")).toHaveText(["1", "2"]);
			await expect(den.locator(".den-count .glyph")).toHaveCount(2);
			await shot(page, `den-eight-${taskbar}-motion-${motion}`);
		});

for (const taskbar of TASKBARS)
	test(`den: eight-den, small, ${taskbar} taskbar`, async ({ page }) => {
		await open(page, `set=eight-den&size=small&taskbar=${taskbar}`);
		await expect(page.getByRole("img", { name: EIGHT_DEN_NAME })).toBeVisible();
		await shot(page, `den-eight-small-${taskbar}`);
	});

test("den: the counts fit inside the den at every size, with all four tiers", async ({
	page,
}) => {
	for (const size of ["small", "medium", "large"]) {
		await open(page, `size=${size}`);
		const den = await page.locator(".den").boundingBox();
		const counts = await page.locator(".den-counts").boundingBox();
		if (!den || !counts) throw new Error("no den");
		expect(counts.x, size).toBeGreaterThanOrEqual(den.x);
		expect(counts.y, size).toBeGreaterThanOrEqual(den.y);
		expect(counts.x + counts.width, size).toBeLessThanOrEqual(
			den.x + den.width,
		);
		expect(counts.y + counts.height, size).toBeLessThanOrEqual(
			den.y + den.height,
		);
	}
});

test("den: only a Needs-you session steps out, and steps back in", async ({
	page,
}) => {
	await open(page, "set=eight-den");
	const den = (name: string) => page.getByRole("img", { name });
	await expect(den(EIGHT_DEN_NAME)).toBeVisible();
	// Ready in the den: it stays in.
	await mock(page, (m) => m.set("s26", { state: "ready", tier: "ready" }));
	await expect(den("3 more sessions: 2 ready, 1 working")).toBeVisible();
	// Needs you: infra gets a pill; api-gateway (the last calm pill) goes into the den.
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
	await expect(page.getByRole("button", { name: /^api-gateway/ })).toHaveCount(
		0,
	);
	await expect(den("3 more sessions: 2 ready, 1 working")).toBeVisible();
	// Back to Working: they swap back.
	await mock(page, (m) =>
		m.set("s26", { state: "working", tier: "working", needs: null }),
	);
	await expect(page.getByRole("button", { name: /^infra/ })).toHaveCount(0);
	await expect(
		page.getByRole("button", { name: "api-gateway, Done, ready to review" }),
	).toBeVisible();
	await expect(den(EIGHT_DEN_NAME)).toBeVisible();
});

// The "While you were away" card (FR-9): left of the strip, in plain words.
const AWAY_NAME =
	"While you were away (25 min): web-shop, Ready to review; data-jobs, Needs you; " +
	"mobile-app, Stopped with an error; docs-site, Ready to review; billing, Needs you; " +
	"and 2 more. Click to close.";

for (const taskbar of TASKBARS)
	test(`away card, ${taskbar} taskbar`, async ({ page }) => {
		await open(page, `set=eight-den&taskbar=${taskbar}&away=7`);
		const card = page.getByRole("button", { name: AWAY_NAME, exact: true });
		await expect(card).toBeVisible();
		await expect(card.locator(".away-head")).toHaveText(
			"While you were away (25 min)",
		);
		await expect(card.locator("li")).toHaveText([
			"web-shopReady to review",
			"data-jobsNeeds you",
			"mobile-appStopped with an error",
			"docs-siteReady to review",
			"billingNeeds you",
		]);
		await expect(card.locator(".away-more")).toHaveText("and 2 more");
		// Narrator announces it.
		await expect(page.getByRole("status")).toHaveText(AWAY_NAME);
		await shot(page, `away-${taskbar}`);
	});

test("away card, small, dark taskbar, a long project name", async ({
	page,
}) => {
	await open(page, "set=eight-den&size=small&taskbar=dark");
	await mock(page, (m) =>
		m.away({
			since: new Date(Date.now() - 95 * 60_000).toISOString(),
			items: [
				{
					id: "s29",
					project: "a-project-with-a-very-long-name",
					outcome: "failed",
				},
				{ id: "s21", project: "web-shop", outcome: "ready" },
			],
		}),
	);
	await expect(page.locator(".away-head")).toHaveText(
		"While you were away (1 h 35 min)",
	);
	// The project is cut short; the outcome never is.
	const clipped = await page
		.locator(".away-outcome")
		.evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth));
	expect(clipped).toHaveLength(0);
	await shot(page, "away-small-dark");
});

test("away card with Animation effects off: no rise, it just shows", async ({
	page,
}) => {
	await open(page, "set=eight-den&taskbar=light&away=3", "off");
	await expect(page.locator(".away")).toHaveCSS("animation-name", "none");
	await shot(page, "away-light-motion-off");
	await open(page, "set=eight-den&taskbar=light&away=3", "on");
	await expect(page.locator(".away")).toHaveCSS("animation-name", "away-in");
});

const lastCall = async (page: Page, cmd: string) =>
	((await page.locator("#log").textContent()) ?? "")
		.split("\n")
		.find((l) => l.startsWith(`${cmd} `));

test("away card: sized into the window, a hit region only while it shows, folds on click", async ({
	page,
}) => {
	await open(page, "show=ready,failed");
	const log = page.locator("#log");
	const regions = async () =>
		JSON.parse((await lastCall(page, "set_hit_regions"))?.slice(16) ?? "{}")
			.regions.length;
	await expect.poll(() => regions()).toBe(3); // two pills and a dismiss button
	await mock(page, (m) => m.away(2));
	await expect(page.locator(".away")).toBeVisible();
	// As wide as the card (264); the 64 strip + 8 gap + 77 card tall.
	await expect
		.poll(() => lastCall(page, "request_strip_size"))
		.toBe('request_strip_size {"width":264,"height":149}');
	// The card sits above the pills, right edges level.
	await page.locator(".away").evaluate((e) => {
		for (const a of e.getAnimations()) a.finish();
	});
	const card = await union(page, ".away");
	const strip = await union(page, ".strip");
	expect(card.y + card.height).toBeLessThanOrEqual(strip.y - 8 + 1);
	expect(card.x + card.width).toBeCloseTo(strip.x + strip.width, 0);
	await expect.poll(() => regions()).toBe(4);
	await page.locator(".away").click();
	await expect(page.locator(".away")).toHaveCount(0);
	await expect(log).toContainText("dismiss_away_summary {}");
	await expect
		.poll(() => lastCall(page, "request_strip_size"))
		.toBe('request_strip_size {"width":128,"height":64}');
	await expect.poll(() => regions()).toBe(3);
	expect(
		(await log.textContent())?.match(/dismiss_away_summary/g),
	).toHaveLength(1);
});

test("away card: folds by itself after 60 s; a newer one replaces it", async ({
	page,
}) => {
	await page.clock.install();
	await open(page, "show=ready");
	const log = page.locator("#log");
	await mock(page, (m) => m.away(7));
	await expect(page.locator(".away li")).toHaveCount(5);
	await page.clock.runFor(30_000);
	await mock(page, (m) => m.away(1));
	await expect(page.locator(".away li")).toHaveCount(1);
	await expect(page.locator(".away-more")).toHaveCount(0);
	await page.clock.runFor(59_000);
	await expect(page.locator(".away")).toBeVisible();
	await expect(log).not.toContainText("dismiss_away_summary");
	await page.clock.runFor(1_100);
	await expect(page.locator(".away")).toHaveCount(0);
	await expect(log).toContainText("dismiss_away_summary {}");
	expect(
		(await log.textContent())?.match(/dismiss_away_summary/g),
	).toHaveLength(1);
});

test("Narrator names, and nothing can take focus", async ({ page }) => {
	await open(page, "show=permission,question,seen,failed,ready");
	for (const name of [
		"knock, Needs permission: PowerShell",
		"website, Has a question",
		"notes, Waiting for you",
		"billing-service, Stopped with an error",
		"docs, Done, ready to review",
	])
		await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
	await expect(
		page.locator(".strip :is([tabindex], button, input, a)"),
	).toHaveCount(0);
});

test("size requests, hit regions, jump and dismiss go to the core", async ({
	page,
}) => {
	await open(page, "show=ready,failed");
	const log = page.locator("#log");
	await expect(log).toContainText(
		'request_strip_size {"width":128,"height":64}',
	);
	// Two pills plus the failed pill's dismiss button.
	await expect(log).toContainText(
		/set_hit_regions \{"regions":\[(\{[^}]*\},){2}\{[^}]*\}\]\}/,
	);
	await page
		.getByRole("button", { name: "docs, Done, ready to review" })
		.click();
	await expect(log).toContainText('jump_to_session {"id":"ready"}');
	await expect(page.getByRole("button", { name: "docs, Idle" })).toBeVisible();
	await page.getByRole("button", { name: "Dismiss billing-service" }).click();
	await expect(log).toContainText('dismiss {"id":"failed"}');
	await expect(page.locator(".strip .slot")).toHaveCount(1);
	await expect(log).toContainText(
		'request_strip_size {"width":64,"height":64}',
	);
});

test("status words never truncate, at any size", async ({ page }) => {
	for (const size of ["small", "medium", "large"])
		for (const show of [SAMPLE_KEYS.slice(0, 5), SAMPLE_KEYS.slice(5)]) {
			await open(page, `size=${size}&show=${show.join(",")}`);
			const clipped = await page
				.locator(".strip .word")
				.evaluateAll((els) =>
					els
						.filter((e) => e.scrollWidth > e.clientWidth)
						.map((e) => e.textContent),
				);
			expect(clipped, size).toEqual([]);
		}
});

test("app and tray icons, 16 to 64 px, light and dark", async ({ page }) => {
	await page.goto("/overlay/dev/icons.html");
	await expect(page.locator("img")).toHaveCount(72);
	await page.waitForFunction(() =>
		[...document.images].every((i) => i.complete && i.naturalWidth > 0),
	);
	await expect(page.locator("#sheet")).toHaveScreenshot("icons.png");
});
