// The settings window (opened by the core). `?setup=1` starts the setup flow (§10.4);
// `?repair=1` starts the Repair flow (§8.1, ADR 0023); `?pack=1` opens the Characters page
// for a .knockpack Knock was opened with (ADR 0038).
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { useStore } from "zustand";
import { call, listen, type Settings } from "../overlay/bridge";
import { createStripStore } from "../overlay/store";
import { AgentsPage } from "./Agents";
import { closeWindow, errorText, invoke, type SetupStatus } from "./core";
import { Characters, type Dropped } from "./Packs";
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
			serverError: null,
			monitorCount: 1,
			stripAvailable: true,
		}),
	);

type View = "setup" | "repair" | "settings" | "packs" | "agents";

function App(props: { status: SetupStatus; view: View }) {
	const settings = useStore(store, (s) => s.settings);
	const [status, setStatus] = useState(props.status);
	const [view, setView] = useState(props.view);
	const [run, setRun] = useState(0);
	const [repairNow, setRepairNow] = useState(false);
	const [dropped, setDropped] = useState<Dropped | null>(null);
	document.title =
		view === "setup"
			? "Set up Knock"
			: view === "repair"
				? "Repair Knock"
				: view === "packs"
					? "Knock Characters"
					: view === "agents"
						? "Knock Agents"
						: "Knock Settings";

	// A .knockpack opened with Knock, or a file dropped on the window, goes to the
	// Characters page, where it is offered. Never in the middle of setup or Repair.
	useEffect(() => {
		const toPacks = () =>
			setView((v) => (v === "settings" || v === "packs" ? "packs" : v));
		void listen<null>("pack-offered", toPacks);
		void listen<{ paths: string[] }>("tauri://drag-drop", (p) => {
			const path = p.paths[0];
			if (!path) return;
			toPacks();
			setDropped((d) => ({ path, n: (d?.n ?? 0) + 1 }));
		});
	}, []);

	// Back on Settings, the status is read again: a repair may have fixed the connection.
	const toSettings = async () => {
		setStatus(await loadStatus());
		setView("settings");
	};
	const open = async (next: View, now = false) => {
		setStatus(await loadStatus());
		setRepairNow(now);
		setRun((r) => r + 1);
		setView(next);
	};

	if (view === "setup" || view === "repair")
		return (
			<Setup
				key={run}
				status={status}
				settings={settings}
				update={update}
				repair={view === "repair"}
				repairNow={repairNow}
				onLeave={() => void toSettings()}
				onFinish={async () => {
					if (!(await closeWindow())) await toSettings();
				}}
			/>
		);
	if (!settings)
		return (
			<main className="settings">
				<p role="status">Loading your settings…</p>
			</main>
		);
	if (view === "agents")
		return (
			<main className="settings">
				<button
					type="button"
					className="link back"
					onClick={() => setView("settings")}
				>
					← Settings
				</button>
				<h1>Other agents</h1>
				<AgentsPage />
			</main>
		);
	if (view === "packs")
		return (
			<main className="settings">
				<button
					type="button"
					className="link back"
					onClick={() => setView("settings")}
				>
					← Settings
				</button>
				<h1>Characters</h1>
				<Characters
					value={settings.pack}
					hidden={settings.hiddenPacks}
					dropped={dropped}
					onChoose={(pack) => update({ pack })}
					onShowHidden={() => update({ hiddenPacks: [] })}
				/>
			</main>
		);
	return (
		<SettingsPage
			settings={settings}
			update={update}
			serverError={status.serverError}
			onRunSetup={() => void open("setup")}
			onRepair={() => void open("repair", true)}
			onOpenCharacters={() => setView("packs")}
			onOpenAgents={() => setView("agents")}
		/>
	);
}

const startView = (q: URLSearchParams): View =>
	q.get("repair") === "1"
		? "repair"
		: q.get("setup") === "1"
			? "setup"
			: q.get("pack") === "1"
				? "packs"
				: "settings";

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
		view={startView(new URLSearchParams(location.search))}
	/>,
);
