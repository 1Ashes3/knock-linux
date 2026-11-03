import { useState } from "react";
import { createRoot } from "react-dom/client";
import { useStore } from "zustand";
import { AnimalStrip } from "./animals/AnimalStrip";
import { call, listen, type SessionsChanged, type Settings } from "./bridge";
import { Strip } from "./Strip";
import { createStripStore, type StripStore } from "./store";
import "./strip.css";

const store = createStripStore();
const {
	applySessions,
	applyInitialSessions,
	applySettings,
	applyInitialSettings,
} = store.getState();

/**
 * FR-3: `settings.mode` picks animals or the plain strip, and a change applies at once.
 * Until settings arrive, and if the animals can't be drawn, the plain strip shows.
 */
function Overlay({ store }: { store: StripStore }) {
	const mode = useStore(store, (s) => s.settings?.mode);
	const [unavailable, setUnavailable] = useState(false);
	if (mode === "animals" && !unavailable)
		return (
			<AnimalStrip store={store} onUnavailable={() => setUnavailable(true)} />
		);
	return <Strip store={store} />;
}

createRoot(document.getElementById("root") as HTMLElement).render(
	<Overlay store={store} />,
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
