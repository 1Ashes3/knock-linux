import { createRoot } from "react-dom/client";
import { call, listen, type SessionsChanged, type Settings } from "./bridge";
import { Strip } from "./Strip";
import { createStripStore } from "./store";
import "./strip.css";

// ponytail: settings mode "animals" also shows the plain strip until the animals land (Phase 3).
const store = createStripStore();
const {
	applySessions,
	applyInitialSessions,
	applySettings,
	applyInitialSettings,
} = store.getState();

createRoot(document.getElementById("root") as HTMLElement).render(
	<Strip store={store} />,
);

// Listen first, then load, so nothing sent in between is lost (§8.5).
await Promise.all([
	listen<SessionsChanged>("sessions-changed", applySessions),
	listen<{ settings: Settings }>("settings-changed", (p) =>
		applySettings(p.settings),
	),
]);
const [sessions, settings] = await Promise.all([
	call("get_sessions"),
	call("get_settings"),
]);
applyInitialSessions(sessions);
applyInitialSettings(settings);
