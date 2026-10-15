import { resolve } from "node:path";
import { defineConfig } from "vite";

// Contract with core-windows: dev on 5173 (strict), overlay at /overlay/index.html,
// build to dist/ with dist/overlay/index.html.
export default defineConfig({
	root: import.meta.dirname,
	server: { port: 5173, strictPort: true },
	build: {
		outDir: "dist",
		emptyOutDir: true,
		rollupOptions: {
			input: { overlay: resolve(import.meta.dirname, "overlay/index.html") },
		},
	},
});
