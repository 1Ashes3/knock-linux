import { defineConfig } from "@playwright/test";

// `pnpm test:visual` (§12.1): screenshots of the strip (overlay/dev/mock.html) and of
// setup and Settings (settings/dev/mock.html), each against a mocked
// bridge. Uses the installed Edge, the same engine as WebView2.
const PORT = 5179; // not 5173, which `pnpm dev` holds

export default defineConfig({
	testDir: ".",
	testMatch: "**/visual/*.visual.ts",
	snapshotPathTemplate: "{testDir}/{testFileDir}/screenshots/{arg}{ext}",
	reporter: "list",
	outputDir: "node_modules/.playwright", // failure diffs; kept out of git
	use: { channel: "msedge", baseURL: `http://localhost:${PORT}` },
	// A handful of pixels for anti-aliasing; a changed word must fail.
	expect: { toHaveScreenshot: { maxDiffPixels: 10 } },
	webServer: {
		command: `pnpm exec vite --port ${PORT} --strictPort`,
		url: `http://localhost:${PORT}/overlay/dev/mock.html`,
		reuseExistingServer: true,
	},
});
