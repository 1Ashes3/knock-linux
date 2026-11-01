// The settings window (opened by the core). `?setup=1` starts the setup flow (§10.4).
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { useStore } from "zustand";
import { call, listen, type Settings } from "../overlay/bridge";
import { createStripStore } from "../overlay/store";
import { closeWindow, errorText, invoke, type SetupStatus } from "./core";
import { SettingsPage } from "./SettingsPage";
import { Setup } from "./Setup";
import "./settings.css";

// Only the settings half of the strip's store is used: it already keeps the newest.
const store = createStripStore();
const { applySettings, applyInitialSettings } = store.getState();

/** The core is the authority: its error, in its words, or null. */
async function update(patch: Partial<Settings>): Promise<string | null> {
	try {
		await invoke("update_settings", { patch });
		return null;
	} catch (e) {
		return errorText(e);
	}
}

const loadStatus = () =>
	invoke<SetupStatus>("setup_status").catch(
		(): SetupStatus => ({
			firstRun: false,
			hooksInstalled: false,
			serverOk: true,
			monitorCount: 1,
		}),
	);

function App(props: { status: SetupStatus; setup: boolean }) {
	const settings = useStore(store, (s) => s.settings);
	const [status, setStatus] = useState(props.status);
	const [view, setView] = useState(props.setup ? "setup" : "settings");
	const [run, setRun] = useState(0);
	document.title = view === "setup" ? "Set up Knock" : "Knock Settings";

	if (view === "setup")
		return (
			<Setup
				key={run}
				status={status}
				settings={settings}
				update={update}
				onLeave={() => setView("settings")}
				onFinish={async () => {
					if (!(await closeWindow())) setView("settings");
				}}
			/>
		);
	if (!settings)
		return (
			<main className="settings">
				<p role="status">Loading your settings…</p>
			</main>
		);
	return (
		<SettingsPage
			settings={settings}
			update={update}
			onRunSetup={async () => {
				setStatus(await loadStatus());
				setRun((r) => r + 1);
				setView("setup");
			}}
		/>
	);
}

// Listen first, then load, so nothing sent in between is lost (§8.5).
await listen<{ settings: Settings }>("settings-changed", (p) =>
	applySettings(p.settings),
);
const [settings, status] = await Promise.all([
	call("get_settings"),
	loadStatus(),
]);
applyInitialSettings(settings);
createRoot(document.getElementById("root") as HTMLElement).render(
	<App
		status={status}
		setup={new URLSearchParams(location.search).get("setup") === "1"}
	/>,
);
