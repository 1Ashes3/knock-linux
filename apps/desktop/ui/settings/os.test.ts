import { describe, expect, it } from "vitest";
import { detectOs } from "./os";

const WEBVIEW2 =
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";
const WEBKITGTK =
	"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

describe("detectOs", () => {
	it("reads the webview's platform", () => {
		expect(detectOs(WEBVIEW2, "")).toBe("windows");
		expect(detectOs(WEBKITGTK, "")).toBe("linux");
		expect(detectOs("", "")).toBe("windows");
	});

	it("?os= pins either one", () => {
		expect(detectOs(WEBKITGTK, "?setup=1&os=windows")).toBe("windows");
		expect(detectOs(WEBVIEW2, "?os=linux")).toBe("linux");
		expect(detectOs(WEBVIEW2, "?os=mac")).toBe("windows");
	});
});
