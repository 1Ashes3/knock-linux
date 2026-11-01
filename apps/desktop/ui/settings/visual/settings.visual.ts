import { expect, type Page, test } from "@playwright/test";

// Setup (§10.4) and Settings against the fake core in settings/dev/mock.html, in the
// Windows light and dark app themes.
const THEMES = ["light", "dark"] as const;
type Theme = (typeof THEMES)[number];

async function open(page: Page, query: string, theme: Theme = "light") {
	await page.setViewportSize({ width: 640, height: 720 });
	await page.emulateMedia({ colorScheme: theme });
	await page.goto(`/settings/dev/mock.html?${query}`);
	await page.addStyleTag({ content: "#log { display: none }" });
	await expect(page.locator("h1")).toBeVisible();
}

const click = (page: Page, name: string) =>
	page.getByRole("button", { name, exact: true }).click();

const heading = (page: Page, name: string) =>
	expect(page.getByRole("heading", { level: 1, name })).toBeVisible();

const calls = (page: Page) =>
	page.evaluate(
		() => (globalThis as unknown as { __calls: [string, unknown][] }).__calls,
	);

/** WCAG AA: every visible text against the background it sits on (§10.5). */
async function expectContrast(page: Page) {
	const low = await page.evaluate(() => {
		const rgb = (c: string) => (c.match(/[\d.]+/g) ?? []).map(Number);
		const lum = (c: number[]) => {
			const [r, g, b] = c
				.slice(0, 3)
				.map((v) => v / 255)
				.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
			return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
		};
		const background = (el: Element | null): number[] => {
			for (let e = el; e; e = e.parentElement) {
				const c = rgb(getComputedStyle(e).backgroundColor);
				if (c.length === 3 || (c[3] ?? 0) > 0) return c;
			}
			return rgb(getComputedStyle(document.documentElement).backgroundColor);
		};
		const out: string[] = [];
		const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
		while (walk.nextNode()) {
			const node = walk.currentNode;
			const el = node.parentElement;
			if (
				!el ||
				!node.textContent?.trim() ||
				el.closest("#log, :disabled, .sr-only")
			)
				continue;
			const a = lum(rgb(getComputedStyle(el).color));
			const b = lum(background(el));
			const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
			if (ratio < 4.5)
				out.push(`${node.textContent.trim()}: ${ratio.toFixed(2)}`);
		}
		return out;
	});
	expect(low).toEqual([]);
}

async function shot(page: Page, name: string) {
	await expectContrast(page);
	await expect(page).toHaveScreenshot(`${name}.png`, { fullPage: true });
}

// The way to each screen from the start of the flow, with two displays.
const TO_SIGNIN = ["Start", "Next", "Confirm and connect", "Next"];
const TO_TEST = [...TO_SIGNIN, "Yes, start with Windows"];
const SCREENS: [string, string, string[]][] = [
	["welcome", "", []],
	["taskbar", "", ["Start"]],
	["connect", "", ["Start", "Next"]],
	["connect-already", "installed=1", ["Start", "Next"]],
	["connect-invalid-json", "preview=invalid", ["Start", "Next"]],
	["connect-write-failed", "apply=fail", TO_SIGNIN.slice(0, 3)],
	["connected", "", TO_SIGNIN.slice(0, 3)],
	["connected-no-backup", "empty=1", TO_SIGNIN.slice(0, 3)],
	["signin", "", TO_SIGNIN],
	["test", "", TO_TEST],
	["test-played", "", [...TO_TEST, "Play a test knock"]],
	["test-missed", "", [...TO_TEST, "Play a test knock", "No, I didn't"]],
	["done", "", [...TO_TEST, "Play a test knock", "Yes, I saw it"]],
];

for (const theme of THEMES) {
	for (const [name, query, path] of SCREENS)
		test(`setup: ${name}, ${theme}`, async ({ page }) => {
			await open(page, `setup=1&${query}`, theme);
			for (const b of path) await click(page, b);
			await expect(page.getByText("Reading your")).toHaveCount(0);
			await shot(page, `setup-${name}-${theme}`);
		});

	test(`setup: the exact change, opened, ${theme}`, async ({ page }) => {
		await open(page, "setup=1&monitors=1", theme);
		await click(page, "Start");
		await page.getByText("Show the exact change").click();
		await expect(page.locator(".diff")).toBeVisible();
		await shot(page, `setup-connect-diff-${theme}`);
	});

	test(`settings, ${theme}`, async ({ page }) => {
		await open(page, "installed=1", theme);
		await expect(page.locator(".checks li")).toHaveCount(3);
		await shot(page, `settings-${theme}`);
	});

	test(`settings: remove, confirm and done, ${theme}`, async ({ page }) => {
		await open(page, "installed=1", theme);
		await expect(page.locator(".checks li")).toHaveCount(3);
		await click(page, "Remove Knock from Claude Code");
		await expect(
			page.getByRole("region", { name: "Remove Knock from Claude Code?" }),
		).toBeVisible();
		await shot(page, `settings-remove-confirm-${theme}`);
		await click(page, "Confirm and remove");
		await expect(
			page.getByText("Knock was removed from Claude Code."),
		).toBeVisible();
		await expect(
			page.getByText("Knock isn't connected, so it can't see"),
		).toBeVisible();
		await shot(page, `settings-removed-${theme}`);
	});

	test(`settings: errors, ${theme}`, async ({ page }) => {
		await open(page, "save=fail", theme);
		await expect(page.locator(".checks li")).toHaveCount(3);
		await page.getByLabel("Remind me after (minutes)").fill("0");
		await page.getByLabel("Quiet from").focus();
		await page.getByLabel("Pause notifications").click();
		await expect(page.getByRole("alert")).toHaveCount(2);
		await page.getByLabel("Quiet from").blur();
		await shot(page, `settings-errors-${theme}`);
	});
}

test("setup skips the taskbar step with one display", async ({ page }) => {
	await open(page, "setup=1&monitors=1");
	await expect(page.getByText("Step 1 of 5")).toBeVisible();
	await click(page, "Start");
	await heading(page, "Connect Knock to Claude Code");
	await expect(page.getByText("Step 2 of 5")).toBeVisible();
});

test("choosing a taskbar saves it right away", async ({ page }) => {
	await open(page, "setup=1");
	await click(page, "Start");
	await page.getByLabel("Display 3: Generic PnP Monitor, 1920 × 1080").check();
	expect(await calls(page)).toContainEqual([
		"update_settings",
		{ patch: { monitor: "monitor-GSM5B7F-3" } },
	]);
	await page.getByLabel("DELL U2720Q (main display), 2560 × 1440").check();
	expect(await calls(page)).toContainEqual([
		"update_settings",
		{ patch: { monitor: "primary" } },
	]);
});

test("no settings file before: says there was nothing to back up", async ({
	page,
}) => {
	await open(page, "setup=1&monitors=1&empty=1");
	await click(page, "Start");
	await expect(
		page.getByText("This file doesn't exist yet, so Knock will create it."),
	).toBeVisible();
	await click(page, "Confirm and connect");
	await heading(page, "Knock is connected to Claude Code");
	await expect(
		page.getByText(
			"There was no settings file yet, so there was nothing to back up.",
		),
	).toBeVisible();
	await expect(page.locator(".path")).toHaveCount(0);
});

test("invalid JSON: the message, Back, and nothing written", async ({
	page,
}) => {
	await open(page, "setup=1&monitors=1&preview=invalid");
	await click(page, "Start");
	await heading(page, "Knock can't change your Claude Code settings yet");
	await expect(page.getByRole("alert")).toContainText("mistake on line 4");
	await expect(page.getByRole("button")).toHaveText(["Back"]);
	await click(page, "Back");
	await heading(page, "Welcome to Knock");
	const names = (await calls(page)).map(([c]) => c);
	expect(names).toContain("setup_preview_hooks");
	expect(names).not.toContain("setup_apply_hooks");
});

test("the whole setup by keyboard alone; focus moves to each new screen", async ({
	page,
}) => {
	await open(page, "setup=1");
	const press = async (name: string) => {
		for (let i = 0; i < 12; i++) {
			await page.keyboard.press("Tab");
			const focused = await page.evaluate(
				() => document.activeElement?.textContent,
			);
			if (focused === name) return page.keyboard.press("Enter");
		}
		throw new Error(`could not reach ${name} with Tab`);
	};
	const focused = (name: string) =>
		expect(page.getByRole("heading", { level: 1, name })).toBeFocused();

	await focused("Welcome to Knock");
	await press("Start");
	await focused("Which taskbar should Knock sit on?");
	await page.keyboard.press("Tab");
	await page.keyboard.press("ArrowDown");
	await press("Next");
	await focused("Connect Knock to Claude Code");
	await press("Confirm and connect");
	await focused("Knock is connected to Claude Code");
	await expect(page.locator(".path")).toHaveText(/backups\\settings\./);
	await press("Next");
	await press("No, I'll start it myself");
	await focused("Try a test knock");
	await press("Play a test knock");
	await expect(page.getByText("Did you see it knock?")).toBeVisible();
	await press("Yes, I saw it");
	await focused("Knock is ready");
	await expect(
		page.getByText(
			"already open connect within about 10 seconds; no restart needed",
		),
	).toBeVisible();
	await expect(
		page.getByText(
			"right-click the fox icon near the clock (you may need the ^ arrow) and choose Settings",
		),
	).toBeVisible();
	await press("Finish");
	// The mock has no window to close, so the app shows Settings instead.
	await heading(page, "Knock Settings");

	const sent = await calls(page);
	expect(sent).toContainEqual([
		"update_settings",
		{ patch: { monitor: "monitor-GSM5B7F-2" } },
	]);
	expect(sent).toContainEqual([
		"update_settings",
		{ patch: { launchAtSignIn: false } },
	]);
	expect(sent.map(([c]) => c)).toEqual(
		expect.arrayContaining(["setup_apply_hooks", "play_test_knock"]),
	);
});

test("Back on every setup screen; Not now opens Settings", async ({ page }) => {
	await open(page, "setup=1");
	for (const b of [...TO_TEST, "Play a test knock", "Yes, I saw it"])
		await click(page, b);
	const titles = [await page.locator("h1").innerText()];
	for (let i = 0; i < 5; i++) {
		await click(page, "Back");
		titles.push(await page.locator("h1").innerText());
	}
	expect(titles).toEqual([
		"Knock is ready",
		"Try a test knock",
		"Start Knock when you sign in?",
		"Knock is connected to Claude Code",
		"Which taskbar should Knock sit on?",
		"Welcome to Knock",
	]);
	await click(page, "Not now");
	await heading(page, "Knock Settings");
});

test("Settings: invalid values are explained and never sent; valid ones are", async ({
	page,
}) => {
	await open(page, "");
	const stall = page.getByLabel("Call a session stuck after (minutes)");
	await stall.fill("1441");
	await stall.press("Enter");
	await expect(page.getByRole("alert")).toHaveText(
		"Enter a whole number of minutes from 1 to 1440 (1440 minutes is one day).",
	);
	await expect(stall).toHaveAttribute("aria-invalid", "true");
	expect((await calls(page)).map(([c]) => c)).not.toContain("update_settings");
	await stall.fill("15");
	await stall.press("Enter");
	await expect(page.getByRole("alert")).toHaveCount(0);
	expect(await calls(page)).toContainEqual([
		"update_settings",
		{ patch: { stallMinutes: 15 } },
	]);

	await page.getByLabel("Quiet hours").check();
	await page.getByLabel("Quiet from").fill("21:30");
	await page.getByLabel("Quiet from").blur();
	expect(await calls(page)).toContainEqual([
		"update_settings",
		{ patch: { quietHours: { enabled: true, start: "21:30", end: "08:00" } } },
	]);
	await page.getByLabel("Large").check();
	await page.getByLabel("Start Knock when I sign in to Windows").check();
	await expect(page.getByLabel("Large")).toBeChecked();
	await expect(
		page.getByLabel("Start Knock when I sign in to Windows"),
	).toBeChecked();
});

test("Settings: every control has a label and a one-line explanation; no sounds control", async ({
	page,
}) => {
	await open(page, "");
	const unexplained = await page.locator("main input").evaluateAll((els) =>
		els
			.filter((el) => {
				const input = el as HTMLInputElement;
				const described = (input.closest("fieldset") ?? input)
					.getAttribute("aria-describedby")
					?.split(" ")
					.some((id) => document.getElementById(id)?.textContent);
				return input.labels?.length === 0 || !described;
			})
			.map((el) => el.outerHTML),
	);
	expect(unexplained).toEqual([]);
	await expect(page.getByLabel(/sound/i)).toHaveCount(0);
	await click(page, "Run setup again");
	await heading(page, "Welcome to Knock");
});
