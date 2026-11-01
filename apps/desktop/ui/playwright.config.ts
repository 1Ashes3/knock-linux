import { defineConfig } from "@playwright/test";

// `pnpm test:visual` (§12.1): screenshots of the strip against the mocked bridge
// (overlay/dev/mock.html). Uses the installed Edge, the same engine as WebView2.
const PORT = 5179; // not 5173, which `pnpm dev` holds

export default defineConfig({
	testDir: "overlay/visual",
	testMatch: "*.visual.ts",
	snapshotPathTemplate: "{testDir}/screenshots/{arg}{ext}",
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
