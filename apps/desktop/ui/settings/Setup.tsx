// The setup flow (SPEC.md §10.4). Plain words on every screen, a reason, and Back.
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
	stepOf,
} from "./flow";

export type Update = (patch: Partial<Settings>) => Promise<string | null>;

interface Props {
	status: SetupStatus;
	settings: Settings | null;
	update: Update;
	/** "Not now" on the first screen. */
	onLeave(): void;
	onFinish(): void;
}

export function Setup({ status, settings, update, onLeave, onFinish }: Props) {
	const [flow, dispatch] = useReducer(
		flowReducer,
		status.monitorCount,
		startFlow,
	);
	const step = stepOf(flow);
	const [error, setError] = useState<string | null>(null);
	const [monitors, setMonitors] = useState<Monitor[]>([]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: an error belongs to the screen it was made on
	useEffect(() => setError(null), [step]);

	useEffect(() => {
		if (status.monitorCount > 1)
			void invoke<Monitor[]>("setup_list_monitors").then(setMonitors, () => {});
	}, [status.monitorCount]);

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
					<p>
						Knock shows each Claude Code session as a small figure on your
						taskbar, near the clock. When a session needs you, it knocks and
						Windows shows one notification. Everything else stays calm.
					</p>
					<p>
						Setup takes about a minute. You won't need to type any commands or
						edit any files, and you can change everything later in Settings.
					</p>
					{!status.serverOk && (
						<p className="error" role="alert">
							Knock couldn't start the connection that Claude Code talks to.
							Setup can still finish, but sessions won't show up until you quit
							Knock and open it again.
						</p>
					)}
				</Screen>
			);

		case "taskbar": {
			const chosen = settings && selectedMonitor(settings.monitor, monitors);
			return (
				<Screen
					counter={counter}
					title="Which taskbar should Knock sit on?"
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
						<legend>
							You have more than one display. Knock rests on one taskbar, near
							the clock. Pick the one you look at most; Knock moves there right
							away.
						</legend>
						{monitors.map((m, i) => (
							<label key={m.id} className="option">
								<input
									type="radio"
									name="monitor"
									checked={chosen === m}
									onChange={() => void choose({ monitor: monitorValue(m) })}
								/>
								{monitorLabel(m, i)}
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
							later from Settings.
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
						<p>
							A copy of the file from before the change was saved here, in case
							you ever need it:
						</p>
						<p className="path">{c.backup}</p>
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
					<p>
						To know when a session needs you, Knock adds a few entries to your
						Claude Code settings file. With them, Claude Code tells Knock when a
						session is working, finishes, or is waiting for you.
					</p>
					<ul>
						<li>
							Knock only learns what kind of moment happened. It never sees your
							messages, your code, or what Claude Code writes.
						</li>
						<li>Everything else in the file stays exactly as it is.</li>
						<li>Knock saves a copy of the file before changing it.</li>
						<li>
							You can remove Knock from Claude Code at any time in Settings.
						</li>
					</ul>
					{c.kind === "loading" ? (
						<p role="status">Reading your Claude Code settings file…</p>
					) : (
						<HookChange preview={c.preview} installed={status.hooksInstalled} />
					)}
				</Screen>
			);
		}

		case "signin":
			return (
				<Screen
					counter={counter}
					title="Start Knock when you sign in?"
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
								Yes, start with Windows
							</button>
						</>
					}
				>
					<p>
						If Knock starts by itself when you sign in to Windows, you won't
						miss a knock after restarting your PC. It uses very little memory
						and stays quiet until a session needs you.
					</p>
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
						So you know what to look for: press the button, then watch the right
						end of your taskbar, near the clock. A test figure knocks and
						Windows shows a notification. It's only a test; none of your
						sessions are involved.
					</p>
					{flow.test === "played" && (
						<p className="question" role="status">
							Did you see it knock?
						</p>
					)}
					{flow.test === "missed" && (
						<div role="status">
							<p>Some things to check:</p>
							<ul>
								<li>Look at the taskbar on the display you chose.</li>
								<li>
									In Windows Settings, under System, then Notifications, make
									sure notifications are on for Knock and Do not disturb is off.
								</li>
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
						seconds; no restart needed. New sessions show up on your taskbar as
						soon as they start.
					</p>
					<p>
						To change anything, or to remove Knock from Claude Code, open
						Knock's Settings.
					</p>
				</Screen>
			);
	}
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
