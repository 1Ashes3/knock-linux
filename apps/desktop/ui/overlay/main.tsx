import { useState } from "react";
import { createRoot } from "react-dom/client";
import { useStore } from "zustand";
import { AnimalStrip } from "./animals/AnimalStrip";
import { createAwayCard } from "./away";
import {
	type AwaySummary,
	call,
	listen,
	type SessionsChanged,
	type Settings,
} from "./bridge";
import { POINTER_ENTERED, POINTER_LEFT } from "./placement";
import { Strip } from "./Strip";
import { createStripStore, type StripStore } from "./store";
import "./strip.css";

const store = createStripStore();
const {
	applySessions,
	applyInitialSessions,
	applySettings,
	applyInitialSettings,
	applyAway,
} = store.getState();
const away = createAwayCard(store);

/**
 * FR-3: `settings.mode` picks animals or the plain strip, and a change applies at once.
 * Until settings arrive, and if the animals can't be drawn, the plain strip shows.
 */
function Overlay({ store }: { store: StripStore }) {
	const mode = useStore(store, (s) => s.settings?.mode);
	const [unavailable, setUnavailable] = useState(false);
	if (mode === "animals" && !unavailable)
		return (
			<AnimalStrip
				store={store}
				onUnavailable={() => setUnavailable(true)}
				onDismissAway={away.dismiss}
			/>
		);
	return <Strip store={store} onDismissAway={away.dismiss} />;
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
	listen<AwaySummary>("away-summary", (p) => applyAway(p)),
	// Linux only (ADR 0037): the pointer left the strip's input region.
	listen<null>("pointer-left", () =>
		window.dispatchEvent(new Event(POINTER_LEFT)),
	),
	listen<null>("pointer-entered", () =>
		window.dispatchEvent(new Event(POINTER_ENTERED)),
	),
]);
const [sessions, settings] = await Promise.all([
	call("get_sessions"),
	call("get_settings"),
]);
applyInitialSessions(sessions);
applyInitialSettings(settings);
