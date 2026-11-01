import { expect, type Page, test } from "@playwright/test";
import { SAMPLE_KEYS } from "../dev/samples";

const TASKBARS = ["light", "dark"] as const;
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

/** The strip plus its margin and the fake taskbar under it. */
async function shot(page: Page, name: string) {
	const box = await page.locator(".strip").boundingBox();
	if (!box) throw new Error("no strip");
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

// Every size with all samples: 5 pills and "+5".
for (const size of ["small", "medium", "large"])
	for (const taskbar of TASKBARS)
		test(`all samples, ${size}, ${taskbar} taskbar`, async ({ page }) => {
			await open(page, `size=${size}&taskbar=${taskbar}`);
			await expect(page.locator(".strip .slot")).toHaveCount(6);
			await shot(page, `all-${size}-${taskbar}`);
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
