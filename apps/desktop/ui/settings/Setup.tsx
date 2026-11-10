// The setup flow (SPEC.md §10.4) and the Repair flow (§8.1, ADR 0023). Plain words on every
// screen, a reason, and Back.
import { type ReactNode, useEffect, useReducer, useRef, useState } from "react";
import type { Settings } from "../overlay/bridge";
import {
	invoke,
	invokeResult,
	type Monitor,
	type PreviewOk,
	type SetupStatus,
	type Written,
} from "./core";
import {
	flowReducer,
	lineDiff,
	monitorLabel,
	monitorValue,
	selectedMonitor,
	startFlow,
	startRepair,
	stepOf,
} from "./flow";
import { WORDS } from "./os";

export type Update = (patch: Partial<Settings>) => Promise<string | null>;

interface Props {
	status: SetupStatus;
	settings: Settings | null;
	update: Update;
	/** "Not now" on the first screen. */
	onLeave(): void;
	onFinish(): void;
	/** Run the Repair flow instead of setup; `repairNow` presses Repair straight away. */
	repair?: boolean;
	repairNow?: boolean;
}

/** ADR 0023: what the Repair screen says when the core gives no reason. */
export const REPAIR_FALLBACK = `Another program on ${WORDS.machine} is using the connection Knock had. Knock can pick a new one.`;

export function Setup({
	status,
	settings,
	update,
	onLeave,
	onFinish,
	repair = false,
	repairNow = false,
}: Props) {
	const [flow, dispatch] = useReducer(flowReducer, undefined, () =>
		repair ? startRepair() : startFlow(status.monitorCount),
	);
	const step = stepOf(flow);
	const hasTaskbar = flow.steps.includes("taskbar");
	const [error, setError] = useState<string | null>(null);
	const [monitors, setMonitors] = useState<Monitor[]>([]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: an error belongs to the screen it was made on
	useEffect(() => setError(null), [step]);

	useEffect(() => {
		if (hasTaskbar)
			void invoke<Monitor[]>("setup_list_monitors").then(setMonitors, () => {});
	}, [hasTaskbar]);

	async function repairServer() {
		dispatch({ type: "repair" });
		const result = await invokeResult<{ ok: true }>("repair_server");
		dispatch({ type: "repaired", result });
	}
	// biome-ignore lint/correctness/useExhaustiveDependencies: once, when opened from the Repair button
	useEffect(() => {
		if (repairNow) void repairServer();
	}, []);

	// §8.2: read and preview first; nothing is written until Confirm.
	const loading = step === "connect" && flow.connect.kind === "loading";
	useEffect(() => {
		if (!loading) return;
		void invokeResult<PreviewOk>("setup_preview_hooks").then((result) =>
			dispatch({ type: "previewed", result }),
		);
	}, [loading]);

	const next = () => dispatch({ type: "next" });
	const back = (
		<button type="button" onClick={() => dispatch({ type: "back" })}>
			Back
		</button>
	);
	const counter = `Step ${flow.at + 1} of ${flow.steps.length}`;

	async function choose(patch: Partial<Settings>, thenNext = false) {
		const e = await update(patch);
		setError(e);
		if (!e && thenNext) next();
	}

	switch (step) {
		case "repair": {
			const r = flow.repair;
			const notNow = (
				<button type="button" onClick={onLeave}>
					Not now
				</button>
			);
			if (r.kind === "repaired")
				return (
					<Screen
						counter={counter}
						title="Knock has a new connection"
						actions={
							<>
								{notNow}
								<button type="button" className="primary" onClick={next}>
									Next
								</button>
							</>
						}
					>
						<p>
							Claude Code's settings still point to the old connection. Press
							Next to see the change that points them to the new one.
						</p>
					</Screen>
				);
			return (
				<Screen
					counter={counter}
					title="Knock can't listen for Claude Code"
					error={r.kind === "failed" ? r.message : null}
					actions={
						<>
							{notNow}
							<button
								type="button"
								className="primary"
								disabled={r.kind === "working"}
								onClick={() => void repairServer()}
							>
								{r.kind === "working"
									? "Repairing…"
									: r.kind === "failed"
										? "Try again"
										: "Repair"}
							</button>
						</>
					}
				>
					<p>{status.serverError || REPAIR_FALLBACK}</p>
					<p>
						Until this is fixed, your sessions can't show up on {WORDS.where}.
						Repair picks a new connection. Then Knock shows you the change this
						needs in your Claude Code settings, and saves it only when you
						confirm.
					</p>
				</Screen>
			);
		}

		case "fixed":
			return (
				<Screen
					counter={counter}
					title="Knock is fixed"
					actions={
						<>
							{back}
							<button type="button" className="primary" onClick={onFinish}>
								Finish
							</button>
						</>
					}
				>
					<p role="status">
						Knock is listening for Claude Code again, and Claude Code's settings
						point to the new connection.
					</p>
					<p>
						Claude Code windows that are already open connect within about 10
						seconds; no restart needed.
					</p>
					{flow.connect.kind === "connected" && (
						<Backup path={flow.connect.backup} />
					)}
				</Screen>
			);

		case "welcome":
			return (
				<Screen
					counter={counter}
					title={status.firstRun ? "Welcome to Knock" : "Set up Knock again"}
					actions={
						<>
							<button type="button" onClick={onLeave}>
								Not now
							</button>
							<button type="button" className="primary" onClick={next}>
								Start
							</button>
						</>
					}
				>
					<p>{WORDS.welcome}</p>
					<p>
						Setup takes about a minute. You won't need to type any commands or
						edit any files, and you can change everything later in Settings.
					</p>
					{!status.serverOk && (
						<p className="error" role="alert">
							Knock couldn't start the connection that Claude Code talks to.
							Choose Not now, then Repair in Settings, then run setup again.
						</p>
					)}
				</Screen>
			);

		case "taskbar": {
			const chosen = settings && selectedMonitor(settings.monitor, monitors);
			return (
				<Screen
					counter={counter}
					title={WORDS.pickTitle}
					error={error}
					actions={
						<>
							{back}
							<button type="button" className="primary" onClick={next}>
								Next
							</button>
						</>
					}
				>
					<fieldset>
						<legend>{WORDS.pickLegend}</legend>
						{monitors.map((m, i) => (
							<label key={m.id} className="option">
								<input
									type="radio"
									name="monitor"
									checked={chosen === m}
									onChange={() => void choose({ monitor: monitorValue(m) })}
								/>
								{monitorLabel(m, i, monitors)}
							</label>
						))}
					</fieldset>
					<p className="help">You can change this later in Settings.</p>
				</Screen>
			);
		}

		case "connect": {
			const c = flow.connect;
			if (c.kind === "failed")
				return (
					<Screen
						counter={counter}
						title={
							c.stage === "preview"
								? "Knock can't change your Claude Code settings yet"
								: "Knock couldn't save the change"
						}
						actions={back}
					>
						<p className="error" role="alert">
							{c.message}
						</p>
						{c.stage === "preview" && (
							<p>Knock hasn't changed anything in your settings file.</p>
						)}
						<p>
							Go Back, then press Next to try again. You can also run setup
							later from Settings: {OPEN_SETTINGS}
						</p>
					</Screen>
				);
			if (c.kind === "connected")
				return (
					<Screen
						counter={counter}
						title="Knock is connected to Claude Code"
						actions={
							<>
								{back}
								<button type="button" className="primary" onClick={next}>
									Next
								</button>
							</>
						}
					>
						<p role="status">Your Claude Code settings file was updated.</p>
						<Backup path={c.backup} />
					</Screen>
				);
			return (
				<Screen
					counter={counter}
					title="Connect Knock to Claude Code"
					actions={
						<>
							{back}
							<button
								type="button"
								className="primary"
								disabled={c.kind === "loading" || c.applying}
								onClick={async () => {
									dispatch({ type: "apply" });
									const result = await invokeResult<Written & { ok: true }>(
										"setup_apply_hooks",
									);
									dispatch({ type: "applied", result });
								}}
							>
								{c.kind === "preview" && c.applying
									? "Connecting…"
									: "Confirm and connect"}
							</button>
						</>
					}
				>
					{repair ? (
						<p>
							Claude Code's settings still point to Knock's old connection.
							Knock updates its own entries in the file to use the new one, so
							Claude Code can tell Knock again when a session is working,
							finishes, or is waiting for you.
						</p>
					) : (
						<p>
							To know when a session needs you, Knock adds a few entries to your
							Claude Code settings file. With them, Claude Code tells Knock when
							a session is working, finishes, or is waiting for you.
						</p>
					)}
					<ul>
						<li>
							Knock only learns what kind of moment happened. It never sees your
							messages, your code, or what Claude Code writes.
						</li>
						<li>Everything else in the file keeps its settings and values.</li>
						<li>Knock saves a copy of the file before changing it.</li>
						<li>
							You can remove Knock from Claude Code at any time in Settings.
						</li>
					</ul>
					{c.kind === "loading" ? (
						<p role="status">Reading your Claude Code settings file…</p>
					) : (
						<HookChange
							preview={c.preview}
							installed={status.hooksInstalled && !repair}
						/>
					)}
				</Screen>
			);
		}

		case "signin":
			return (
				<Screen
					counter={counter}
					title={WORDS.signInTitle}
					error={error}
					actions={
						<>
							{back}
							<button
								type="button"
								onClick={() => void choose({ launchAtSignIn: false }, true)}
							>
								No, I'll start it myself
							</button>
							<button
								type="button"
								className="primary"
								onClick={() => void choose({ launchAtSignIn: true }, true)}
							>
								{WORDS.signInYes}
							</button>
						</>
					}
				>
					<p>{WORDS.signInBody}</p>
					<p className="help">You can change this later in Settings.</p>
				</Screen>
			);

		case "test":
			return (
				<Screen
					counter={counter}
					title="Try a test knock"
					error={error}
					actions={
						<>
							{back}
							{flow.test === "ready" ? (
								<>
									<button type="button" onClick={next}>
										Skip the test
									</button>
									<button
										type="button"
										className="primary"
										onClick={async () => {
											try {
												await invoke("play_test_knock");
												dispatch({ type: "test", result: "played" });
											} catch {
												setError(
													"Knock couldn't play the test knock. You can skip it and carry on.",
												);
											}
										}}
									>
										Play a test knock
									</button>
								</>
							) : flow.test === "played" ? (
								<>
									<button
										type="button"
										onClick={() => dispatch({ type: "test", result: "missed" })}
									>
										No, I didn't
									</button>
									<button type="button" className="primary" onClick={next}>
										Yes, I saw it
									</button>
								</>
							) : (
								<>
									<button
										type="button"
										onClick={() => dispatch({ type: "test", result: "ready" })}
									>
										Try again
									</button>
									<button type="button" className="primary" onClick={next}>
										Carry on anyway
									</button>
								</>
							)}
						</>
					}
				>
					<p>
						So you know what to look for: press the button, then watch{" "}
						{WORDS.testLook}. A test figure knocks and {WORDS.notifier} shows a
						notification. It's only a test; none of your sessions are involved.
					</p>
					{!status.stripAvailable && (
						<p className="help">
							This desktop can't show Knock's figures on screen, so you'll get
							the tray icon and notifications only.
						</p>
					)}
					{flow.test === "played" && (
						<p className="question" role="status">
							Did you see it knock?
						</p>
					)}
					{flow.test === "missed" && (
						<div role="status">
							<p>Some things to check:</p>
							<ul>
								<li>{WORDS.missedPlace}</li>
								<li>{WORDS.missedNotifications}</li>
							</ul>
						</div>
					)}
				</Screen>
			);

		case "done":
			return (
				<Screen
					counter={counter}
					title="Knock is ready"
					actions={
						<>
							{back}
							<button type="button" className="primary" onClick={onFinish}>
								Finish
							</button>
						</>
					}
				>
					<p>
						Claude Code windows that are already open connect within about 10
						seconds; no restart needed. New sessions show up on {WORDS.where} as
						soon as they start.
					</p>
					<p>
						To change anything, or to remove Knock from Claude Code, open
						Settings: {OPEN_SETTINGS}
					</p>
				</Screen>
			);
	}
}

/** Settings opens from the tray menu; the icon may sit in the overflow behind the ^ arrow. */
export const OPEN_SETTINGS = WORDS.openSettings;

/** Where the core saved the old file. An empty path means there was no file to back up (§8.2). */
export function Backup({ path }: { path: string }) {
	return path ? (
		<>
			<p>
				A copy of the file from before the change was saved here, in case you
				ever need it:
			</p>
			<p className="path">{path}</p>
		</>
	) : (
		<p>There was no settings file yet, so there was nothing to back up.</p>
	);
}

/** One setup screen. The heading takes focus on each new screen, so Narrator reads it. */
function Screen({
	counter,
	title,
	error,
	actions,
	children,
}: {
	counter: string;
	title: string;
	error?: string | null;
	actions: ReactNode;
	children: ReactNode;
}) {
	const heading = useRef<HTMLHeadingElement>(null);
	// biome-ignore lint/correctness/useExhaustiveDependencies: a new title is a new screen
	useEffect(() => heading.current?.focus(), [title]);
	return (
		<main className="screen">
			<p className="counter">{counter}</p>
			<h1 ref={heading} tabIndex={-1}>
				{title}
			</h1>
			<div className="body">{children}</div>
			{error && (
				<p className="error" role="alert">
					{error}
				</p>
			)}
			<div className="actions">{actions}</div>
		</main>
	);
}

/** The file path and the exact change, line by line, in an expandable section (§8.2 item 2). */
export function HookChange({
	preview,
	installed,
}: {
	preview: PreviewOk;
	installed: boolean;
}) {
	const diff = lineDiff(preview.before, preview.after);
	const added = diff.filter((l) => l.kind === "add").length;
	const removed = diff.filter((l) => l.kind === "del").length;
	return (
		<div className="change">
			<p>
				The file is <span className="path">{preview.path}</span>
			</p>
			{preview.before.trim() === "" && (
				<p>This file doesn't exist yet, so Knock will create it.</p>
			)}
			{installed && (
				<p>
					Knock is already connected. Confirming replaces Knock's own entries
					with fresh ones and leaves everything else alone.
				</p>
			)}
			<details>
				<summary>
					Show the exact change ({added} {added === 1 ? "line" : "lines"} added,{" "}
					{removed} removed)
				</summary>
				<p className="help">
					Lines marked + are added and lines marked − are removed. The secret
					key that lets only Claude Code talk to Knock is hidden as ••••.
				</p>
				<pre className="diff">
					{diff.map((l, i) => (
						<span
							// biome-ignore lint/suspicious/noArrayIndexKey: a fixed list, rendered once
							key={i}
							className={l.kind}
						>
							{l.kind === "add" ? "+ " : l.kind === "del" ? "− " : "  "}
							{l.text}
							{"\n"}
						</span>
					))}
				</pre>
			</details>
		</div>
	);
}
