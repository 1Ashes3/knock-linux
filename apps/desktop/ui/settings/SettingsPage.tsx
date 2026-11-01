// The Settings window (SPEC.md §8.6, §10.4). Every control explains itself in one line.
// Changes save as soon as they're made; the core is the authority and its errors show.
// `sounds` has no control: sounds aren't in v1.
import { type ReactNode, useEffect, useId, useState } from "react";
import type { Settings, Size } from "../overlay/bridge";
import {
	type Check,
	invoke,
	invokeResult,
	type Monitor,
	type SetupStatus,
	type Written,
} from "./core";
import {
	monitorLabel,
	monitorValue,
	type Parsed,
	parseMinutes,
	parseTime,
	quietHoursPatch,
	selectedMonitor,
} from "./flow";
import { Backup, type Update } from "./Setup";

interface Props {
	settings: Settings;
	update: Update;
	/** From `setup_status()`: why Knock can't listen for Claude Code, or null (ADR 0023). */
	serverError: string | null;
	onRunSetup(): void;
	onRepair(): void;
}

export function SettingsPage({
	settings: s,
	update,
	serverError,
	onRunSetup,
	onRepair,
}: Props) {
	const [monitors, setMonitors] = useState<Monitor[]>([]);
	useEffect(() => {
		void invoke<Monitor[]>("setup_list_monitors").then(setMonitors, () => {});
	}, []);
	const chosen = selectedMonitor(s.monitor, monitors);

	return (
		<main className="settings">
			<h1>Knock Settings</h1>

			{serverError !== null && (
				<section className="banner" aria-labelledby="repair-title">
					<h2 id="repair-title">Knock can't listen for Claude Code</h2>
					<p role="alert">{serverError}</p>
					<p>
						Your sessions can't show up until this is fixed. Repair picks a new
						connection, then shows you the change to your Claude Code settings
						before saving it.
					</p>
					<div className="actions start">
						<button type="button" className="primary" onClick={onRepair}>
							Repair
						</button>
					</div>
				</section>
			)}

			<section aria-labelledby="look">
				<h2 id="look">On the taskbar</h2>
				{monitors.length > 1 && (
					<Choice
						legend="Taskbar"
						help="Which display's taskbar Knock rests on."
						options={monitors.map((m, i) => ({
							value: monitorValue(m),
							label: monitorLabel(m, i, monitors),
						}))}
						value={chosen ? monitorValue(chosen) : ""}
						onChange={(monitor) => update({ monitor })}
					/>
				)}
				<Choice
					legend="Show sessions as"
					help="Animals arrive in a later version. Until then, Knock shows plain labels either way."
					options={[
						{ value: "animals", label: "Animals" },
						{ value: "plain", label: "Plain labels" },
					]}
					value={s.mode}
					onChange={(mode) => update({ mode: mode as Settings["mode"] })}
				/>
				<Choice
					legend="Size"
					help="How big Knock's figures are on the taskbar."
					options={[
						{ value: "small", label: "Small" },
						{ value: "medium", label: "Medium" },
						{ value: "large", label: "Large" },
					]}
					value={s.size}
					onChange={(size) => update({ size: size as Size })}
				/>
			</section>

			<section aria-labelledby="notify">
				<h2 id="notify">Notifications</h2>
				<Toggle
					label="Pause notifications"
					help="Stops all Windows notifications from Knock. The taskbar still shows everything."
					checked={s.pauseNotifications}
					onChange={(pauseNotifications) => update({ pauseNotifications })}
				/>
				<TextField
					label="Remind me after (minutes)"
					help="If a session is still waiting for you after this long, Knock reminds you once."
					type="number"
					value={String(s.escalationMinutes)}
					parse={parseMinutes}
					onCommit={(escalationMinutes) => update({ escalationMinutes })}
				/>
				<TextField
					label="Call a session stuck after (minutes)"
					help="A working session with no progress for this long is shown as possibly stuck."
					type="number"
					value={String(s.stallMinutes)}
					parse={parseMinutes}
					onCommit={(stallMinutes) => update({ stallMinutes })}
				/>
				<Toggle
					label="Notify me when a session may be stuck"
					help="Off by default, because a long task can look stuck when it isn't."
					checked={s.notifyOnStall}
					onChange={(notifyOnStall) => update({ notifyOnStall })}
				/>
				<Toggle
					label="Quiet hours"
					help="No notifications between the times below, every day. The taskbar still shows everything."
					checked={s.quietHours.enabled}
					onChange={(enabled) =>
						update(quietHoursPatch(s.quietHours, { enabled }))
					}
				/>
				<div className="pair">
					<TextField
						label="Quiet from"
						help="When quiet hours begin each day."
						type="time"
						value={s.quietHours.start}
						parse={parseTime}
						onCommit={(start) =>
							update(quietHoursPatch(s.quietHours, { start }))
						}
					/>
					<TextField
						label="Quiet until"
						help="When notifications come back."
						type="time"
						value={s.quietHours.end}
						parse={parseTime}
						onCommit={(end) => update(quietHoursPatch(s.quietHours, { end }))}
					/>
				</div>
			</section>

			<section aria-labelledby="start">
				<h2 id="start">Starting Knock</h2>
				<Toggle
					label="Start Knock when I sign in to Windows"
					help="So you don't miss a knock after restarting your PC."
					checked={s.launchAtSignIn === true}
					onChange={(launchAtSignIn) => update({ launchAtSignIn })}
				/>
			</section>

			<Connection onRunSetup={onRunSetup} />
			<Health onRepair={onRepair} />
		</main>
	);
}

/** Runs `commit`, shows the core's error if it returns one. */
function useCommit() {
	const [error, setError] = useState<string | null>(null);
	const run = async (commit: Promise<string | null>) => setError(await commit);
	return [error, run] as const;
}

function Help({ id, text }: { id: string; text: string }) {
	return (
		<p id={id} className="help">
			{text}
		</p>
	);
}

function FieldError({ id, error }: { id: string; error: string | null }) {
	return error ? (
		<p id={id} className="error" role="alert">
			{error}
		</p>
	) : null;
}

function Choice({
	legend,
	help,
	options,
	value,
	onChange,
}: {
	legend: string;
	help: string;
	options: { value: string; label: string }[];
	value: string;
	onChange(value: string): Promise<string | null>;
}) {
	const id = useId();
	const [error, run] = useCommit();
	return (
		<fieldset className="field" aria-describedby={`${id}h ${id}e`}>
			<legend>{legend}</legend>
			<Help id={`${id}h`} text={help} />
			<div className="options">
				{options.map((o) => (
					<label key={o.value} className="option">
						<input
							type="radio"
							name={id}
							value={o.value}
							checked={value === o.value}
							onChange={() => void run(onChange(o.value))}
						/>
						{o.label}
					</label>
				))}
			</div>
			<FieldError id={`${id}e`} error={error} />
		</fieldset>
	);
}

function Toggle({
	label,
	help,
	checked,
	onChange,
}: {
	label: string;
	help: string;
	checked: boolean;
	onChange(checked: boolean): Promise<string | null>;
}) {
	const id = useId();
	const [error, run] = useCommit();
	return (
		<div className="field toggle">
			<input
				id={id}
				type="checkbox"
				checked={checked}
				aria-describedby={`${id}h ${id}e`}
				onChange={(e) => void run(onChange(e.target.checked))}
			/>
			<div>
				<label htmlFor={id}>{label}</label>
				<Help id={`${id}h`} text={help} />
				<FieldError id={`${id}e`} error={error} />
			</div>
		</div>
	);
}

/** Saves on Enter or when focus leaves, and only a valid value. */
function TextField<T>({
	label,
	help,
	type,
	value,
	parse,
	onCommit,
}: {
	label: string;
	help: string;
	type: "number" | "time";
	value: string;
	parse(text: string): Parsed<T>;
	onCommit(value: T): Promise<string | null>;
}) {
	const id = useId();
	const [draft, setDraft] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	async function commit() {
		if (draft === null) return;
		const p = parse(draft);
		if ("error" in p) return setError(p.error);
		const e = await onCommit(p.value);
		setError(e);
		if (!e) setDraft(null);
	}
	return (
		<div className="field">
			<label htmlFor={id}>{label}</label>
			<Help id={`${id}h`} text={help} />
			<input
				id={id}
				type={type}
				{...(type === "number" ? { min: 1, max: 1440, step: 1 } : {})}
				value={draft ?? value}
				aria-invalid={error ? true : undefined}
				aria-describedby={`${id}h ${id}e`}
				onChange={(e) => setDraft(e.target.value)}
				onBlur={() => void commit()}
				onKeyDown={(e) => {
					if (e.key === "Enter") void commit();
				}}
			/>
			<FieldError id={`${id}e`} error={error} />
		</div>
	);
}

type Removal =
	| { kind: "idle" }
	| { kind: "confirm" }
	| { kind: "working" }
	| { kind: "done"; result: Written };

/** §10.4: Remove Knock from Claude Code is always one click away. */
function Connection({ onRunSetup }: { onRunSetup(): void }) {
	const [installed, setInstalled] = useState<boolean | null>(null);
	const [removal, setRemoval] = useState<Removal>({ kind: "idle" });
	const refresh = () =>
		void invoke<SetupStatus>("setup_status").then(
			(st) => setInstalled(st.hooksInstalled),
			() => setInstalled(null),
		);
	useEffect(refresh, []);

	async function remove() {
		setRemoval({ kind: "working" });
		const result = await invokeResult<Written & { ok: true }>(
			"setup_remove_hooks",
		);
		setRemoval({ kind: "done", result });
		refresh();
	}

	let body: ReactNode;
	if (removal.kind === "confirm" || removal.kind === "working")
		body = (
			<section className="panel" aria-labelledby="remove-title">
				<h3 id="remove-title">Remove Knock from Claude Code?</h3>
				<ul>
					<li>
						Knock deletes only its own entries from your Claude Code settings
						file. Everything else in the file keeps its settings and values.
					</li>
					<li>Knock saves a copy of the file before changing it.</li>
					<li>
						Afterwards Knock can't see your sessions, until you run setup again.
					</li>
				</ul>
				<div className="actions">
					<button
						type="button"
						disabled={removal.kind === "working"}
						onClick={() => setRemoval({ kind: "idle" })}
					>
						Back
					</button>
					<button
						type="button"
						className="primary"
						disabled={removal.kind === "working"}
						onClick={() => void remove()}
					>
						{removal.kind === "working" ? "Removing…" : "Confirm and remove"}
					</button>
				</div>
			</section>
		);
	else
		body = (
			<>
				{removal.kind === "done" &&
					(removal.result.ok ? (
						<div role="status">
							<p>Knock was removed from Claude Code.</p>
							<Backup path={removal.result.backup} />
						</div>
					) : (
						<p className="error" role="alert">
							{removal.result.message}
						</p>
					))}
				<div className="actions start">
					<button type="button" onClick={() => setRemoval({ kind: "confirm" })}>
						Remove Knock from Claude Code
					</button>
					<button type="button" onClick={onRunSetup}>
						Run setup again
					</button>
				</div>
			</>
		);

	return (
		<section aria-labelledby="claude">
			<h2 id="claude">Claude Code connection</h2>
			<p className="help">
				{installed === null
					? "Knock couldn't tell whether it's connected."
					: installed
						? "Knock is connected: Claude Code tells it when a session needs you."
						: "Knock isn't connected, so it can't see your sessions. Run setup to connect it."}
			</p>
			{body}
		</section>
	);
}

/** FR-12: the checks and results come from the core, in plain language. */
function Health({ onRepair }: { onRepair(): void }) {
	const [checks, setChecks] = useState<Check[] | null>(null);
	const [error, setError] = useState<string | null>(null);
	const run = () => {
		setChecks(null);
		setError(null);
		invoke<Check[]>("run_health_check").then(setChecks, () =>
			setError("Knock couldn't run the check. Please try again."),
		);
	};
	useEffect(run, []);
	return (
		<section aria-labelledby="health">
			<h2 id="health">Check that Knock is working</h2>
			<p className="help">
				Knock checks its connection to Claude Code and when it last heard from a
				session.
			</p>
			{error ? (
				<p className="error" role="alert">
					{error}
				</p>
			) : checks === null ? (
				<p role="status">Checking…</p>
			) : (
				<ul className="checks" role="status">
					{checks.map((c) => (
						<li key={c.id} className={c.ok ? "ok" : "bad"}>
							<span className="mark" aria-hidden="true">
								{c.ok ? "✓" : "✗"}
							</span>
							<span>
								<strong>
									{c.label}
									<span className="sr-only">
										{c.ok ? ": working" : ": problem"}
									</span>
								</strong>
								<span className="detail">{c.detail}</span>
								{c.id === "server" && !c.ok && (
									<button type="button" className="link" onClick={onRepair}>
										Repair
									</button>
								)}
							</span>
						</li>
					))}
				</ul>
			)}
			<div className="actions start">
				<button
					type="button"
					onClick={run}
					disabled={checks === null && !error}
				>
					Check again
				</button>
			</div>
		</section>
	);
}
