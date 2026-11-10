// Words that differ between Windows and Linux (ADR 0037). The webview tells them apart:
// WebView2 reports Windows, WebKitGTK reports Linux. `?os=windows` or `?os=linux` in the
// page URL overrides it, so the visual tests can pin either.

export type Os = "windows" | "linux";

export function detectOs(userAgent: string, search: string): Os {
	const forced = new URLSearchParams(search).get("os");
	if (forced === "windows" || forced === "linux") return forced;
	if (/Windows/.test(userAgent)) return "windows";
	return /Linux|X11/.test(userAgent) ? "linux" : "windows";
}

export interface Words {
	/** Where the figures are, after "on" or "in": "the taskbar". */
	where: string;
	/** Setup's welcome, first paragraph. */
	welcome: string;
	/** The display step's title and its explanation. */
	pickTitle: string;
	pickLegend: string;
	/** Settings: the section heading, the display choice and its notes. */
	lookHeading: string;
	pickLabel: string;
	pickHelp: string;
	oneDisplay: string;
	/** "Windows shows a notification" and friends. */
	notifier: string;
	notifications: string;
	/** Launch at sign-in. */
	signInTitle: string;
	signInYes: string;
	signInBody: string;
	signInToggle: string;
	signInHelp: string;
	/** Setup's test knock: where to look, and what to check if it was missed. */
	testLook: string;
	missedPlace: string;
	missedNotifications: string;
	/** How to reach Settings from the tray. */
	openSettings: string;
	/** "this PC" or "this computer". */
	machine: string;
}

const WINDOWS: Words = {
	where: "the taskbar",
	welcome:
		"Knock shows each Claude Code session as a small sprite on your taskbar, near the clock. When a session needs you, it knocks and Windows shows one notification. Everything else stays calm.",
	pickTitle: "Which taskbar should Knock sit on?",
	pickLegend:
		"You have more than one display. Knock rests on one taskbar, near the clock. Pick the one you look at most; Knock moves there right away.",
	lookHeading: "On the taskbar",
	pickLabel: "Which taskbar",
	pickHelp:
		"Knock rests on one display's taskbar, near the clock. It moves as soon as you pick.",
	oneDisplay: "Only one display is connected, so Knock uses its taskbar.",
	notifier: "Windows",
	notifications: "Windows notifications",
	signInTitle: "Start Knock when you sign in?",
	signInYes: "Yes, start with Windows",
	signInBody:
		"If Knock starts by itself when you sign in to Windows, you won't miss a knock after restarting your PC. It stays quiet until a session needs you.",
	signInToggle: "Start Knock when I sign in to Windows",
	signInHelp: "So you don't miss a knock after restarting your PC.",
	testLook: "the right end of your taskbar, near the clock",
	missedPlace: "Look at the taskbar on the display you chose.",
	missedNotifications:
		"In Windows Settings, under System, then Notifications, make sure notifications are on for Knock and Do not disturb is off.",
	openSettings:
		"right-click the fox icon near the clock (you may need the ^ arrow) and choose Settings.",
	machine: "this PC",
};

const LINUX: Words = {
	where: "your screen",
	welcome:
		"Knock shows each Claude Code session as a small sprite in the bottom-right corner of your screen, just above your panel. When a session needs you, it knocks and your desktop shows one notification. Everything else stays calm.",
	pickTitle: "Which display should Knock sit on?",
	pickLegend:
		"You have more than one display. Knock rests in the bottom-right corner of one of them. Pick the one you look at most; Knock moves there right away.",
	lookHeading: "On screen",
	pickLabel: "Which display",
	pickHelp:
		"Knock rests in the bottom-right corner of one display. It moves as soon as you pick.",
	oneDisplay: "Only one display is connected, so Knock uses it.",
	notifier: "your desktop",
	notifications: "desktop notifications",
	signInTitle: "Start Knock when you log in?",
	signInYes: "Yes, start when I log in",
	signInBody:
		"If Knock starts by itself when you log in, you won't miss a knock after restarting your computer. It stays quiet until a session needs you.",
	signInToggle: "Start Knock when I log in",
	signInHelp: "So you don't miss a knock after restarting your computer.",
	testLook: "the bottom-right corner of your screen, just above your panel",
	missedPlace: "Look at the bottom-right corner of the display you chose.",
	missedNotifications:
		"In your desktop's settings, under Notifications, make sure notifications are on for Knock and Do not disturb is off.",
	openSettings:
		"click the fox icon in your system tray, near the clock (you may need to open the tray's arrow), and choose Settings.",
	machine: "this computer",
};

export const OS: Os = detectOs(
	globalThis.navigator?.userAgent ?? "",
	globalThis.location?.search ?? "",
);

export const WORDS: Words = OS === "linux" ? LINUX : WINDOWS;
