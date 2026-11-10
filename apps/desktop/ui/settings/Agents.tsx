// Settings → Other agents (ADR 0039): connect Knock to Codex, GitHub Copilot and
// Antigravity. The same promise as for Claude Code: show the exact change, save a
// copy first, write only after the user confirms, and removing takes out only Knock's
// own entries.
import {
	type MouseEvent,
	type ReactNode,
	type RefObject,
	useCallback,
	useEffect,
	useId,
	useRef,
	useState,
} from "react";
import { errorText, invoke } from "./core";
import { lineDiff } from "./flow";
import { usePanelFocus } from "./focus";
import { Backup } from "./Setup";

/** `agents_status`: one line per agent. */
export interface AgentStatus {
	id: string;
	name: string;
	/** Its program or folder is on this computer. */
	found: boolean;
	/** `null` when its file can't be read or changed; `problem` says why. */
	state: "installed" | "stale" | "missing" | null;
	problem: string | null;
	/** What to do once connected. */
	note: string;
}

type Preview =
	| { ok: true; path: string; before: string; after: string; deletes: boolean }
	| { ok: false; message: string };
type Applied =
	| { ok: true; backup: string; note: string }
	| { ok: false; message: string };

/** What the user is looking at: the change they're asked to confirm, or what happened. */
type Step =
	| { kind: "idle" }
	| { kind: "asking"; agent: AgentStatus; remove: boolean; preview: Preview }
	| { kind: "done"; agent: AgentStatus; remove: boolean; result: Applied };

/**
 * How far each agent works. Codex and Copilot are built but haven't been tried against the
 * real programs, so Settings holds them back; Antigravity works in part.
 */
const SUPPORT: Record<string, { label: string; detail: string } | undefined> = {
	agy: {
		label: "Beta, partly working",
		detail:
			"Shows when Antigravity is working and when it finishes. It can't tell when Antigravity is waiting for your permission.",
	},
	codex: {
		label: "Not available yet",
		detail: "Not tested with the real Codex yet.",
	},
	copilot: {
		label: "Not available yet",
		detail: "Not tested with the real GitHub Copilot CLI yet.",
	},
};

/** Only Antigravity can be connected for now. */
const available = (id: string) => id === "agy";

const STATE_WORDS = {
	installed: "Connected",
	stale: "Needs updating",
	missing: "Not connected",
} as const;

export function AgentsPage() {
	const id = useId();
	const [agents, setAgents] = useState<AgentStatus[] | null>(null);
	const [step, setStep] = useState<Step>({ kind: "idle" });
	const [working, setWorking] = useState(false);
	const [error, setError] = useState<string | null>(null);
	/** The button that opened the question; focus goes back to it (§10.5). */
	const opener = useRef<HTMLElement | null>(null);

	const load = useCallback(
		() =>
			void invoke<AgentStatus[]>("agents_status").then(setAgents, (e) =>
				setError(errorText(e)),
			),
		[],
	);
	useEffect(load, [load]);

	const ask = async (
		agent: AgentStatus,
		remove: boolean,
		from: HTMLElement,
	) => {
		opener.current = from;
		setError(null);
		setWorking(true);
		try {
			const preview = await invoke<Preview>("agent_preview", {
				id: agent.id,
				remove,
			});
			setStep({ kind: "asking", agent, remove, preview });
		} catch (e) {
			setError(errorText(e));
		}
		setWorking(false);
	};

	const confirm = async (agent: AgentStatus, remove: boolean) => {
		setError(null);
		setWorking(true);
		try {
			const result = await invoke<Applied>("agent_apply", {
				id: agent.id,
				remove,
			});
			setStep({ kind: "done", agent, remove, result });
			load();
		} catch (e) {
			setError(errorText(e));
		}
		setWorking(false);
	};

	return (
		<section aria-labelledby={`${id}t`}>
			<h2 id={`${id}t`}>Agents</h2>
			<p className="help">
				Knock always watches Claude Code. Antigravity is a beta, and the others
				aren't available yet. Connecting adds Knock's hooks to the agent's own
				hooks file: Knock shows you the change, saves a copy of the file first,
				and writes only when you confirm.
			</p>
			{step.kind === "asking" && (
				<Change
					step={step}
					working={working}
					opener={opener}
					onConfirm={() => void confirm(step.agent, step.remove)}
					onCancel={() => setStep({ kind: "idle" })}
				/>
			)}
			{step.kind === "done" && (
				<Done step={step} onClose={() => setStep({ kind: "idle" })} />
			)}
			{agents === null ? (
				<p className="help">Looking for agents…</p>
			) : (
				<ul className="agents">
					{agents.map((a) => (
						<AgentRow
							key={a.id}
							agent={a}
							busy={working || step.kind === "asking"}
							onConnect={(e) => void ask(a, false, e.currentTarget)}
							onRemove={(e) => void ask(a, true, e.currentTarget)}
						/>
					))}
				</ul>
			)}
			{error && (
				<p className="error" role="alert">
					{error}
				</p>
			)}
		</section>
	);
}

function AgentRow({
	agent: a,
	busy,
	onConnect,
	onRemove,
}: {
	agent: AgentStatus;
	busy: boolean;
	onConnect(e: MouseEvent<HTMLElement>): void;
	onRemove(e: MouseEvent<HTMLElement>): void;
}) {
	const state = a.state;
	const support = SUPPORT[a.id];
	const held = !available(a.id) && state !== "installed";
	return (
		<li
			className="agent"
			data-found={a.found ? "" : undefined}
			data-held={held ? "" : undefined}
		>
			<span className="agent-text">
				<strong>
					{a.name}
					{support && <span className="badge">{support.label}</span>}
				</strong>
				{held ? (
					<span className="help">{support?.detail}</span>
				) : (
					<>
						<span className="help">
							{state ? STATE_WORDS[state] : "Can't be changed"}
							{!a.found &&
								state !== "installed" &&
								" · not found on this computer"}
						</span>
						{support && <span className="help">{support.detail}</span>}
					</>
				)}
				{a.problem && !held && <span className="error">{a.problem}</span>}
			</span>
			<span className="agent-actions">
				{state !== "installed" && (
					<button
						type="button"
						className={state === "stale" ? "primary" : undefined}
						disabled={
							held ||
							busy ||
							state === null ||
							(!a.found && state === "missing")
						}
						onClick={onConnect}
					>
						{state === "stale" ? "Update…" : "Connect…"}
					</button>
				)}
				{state === "installed" && (
					<button type="button" disabled={busy} onClick={onConnect}>
						Reconnect…
					</button>
				)}
				{(state === "installed" || state === "stale") && (
					<button type="button" disabled={busy} onClick={onRemove}>
						Remove…
					</button>
				)}
			</span>
		</li>
	);
}

/** "Connect X?": the file and the exact change, then Confirm or Cancel. */
function Change({
	step,
	working,
	opener,
	onConfirm,
	onCancel,
}: {
	step: Extract<Step, { kind: "asking" }>;
	working: boolean;
	/** Where focus goes back to when the question closes. */
	opener: RefObject<HTMLElement | null>;
	onConfirm(): void;
	onCancel(): void;
}) {
	const { agent, remove, preview } = step;
	// Keyboard focus moves to the question, and back when it closes (§10.5).
	const box = useRef<HTMLFieldSetElement>(null);
	usePanelFocus(box, opener);
	let body: ReactNode;
	if (!preview.ok) {
		body = <p role="alert">{preview.message}</p>;
	} else {
		const diff = lineDiff(preview.before, preview.after);
		const added = diff.filter((l) => l.kind === "add").length;
		const removed = diff.filter((l) => l.kind === "del").length;
		body = (
			<>
				<p>
					The file is <span className="path">{preview.path}</span>
				</p>
				{preview.before.trim() === "" && !remove && (
					<p>This file doesn't exist yet, so Knock will create it.</p>
				)}
				{preview.deletes && (
					<p>This file holds only Knock's hooks, so Knock will delete it.</p>
				)}
				<details>
					<summary>
						Show the exact change ({added} {added === 1 ? "line" : "lines"}{" "}
						added, {removed} removed)
					</summary>
					<p className="help">
						Lines marked + are added and lines marked − are removed.
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
			</>
		);
	}
	return (
		<fieldset className="offer" ref={box} tabIndex={-1}>
			<legend>
				{remove ? "Remove Knock from" : "Connect Knock to"} {agent.name}?
			</legend>
			{body}
			<div className="actions start">
				{preview.ok && (
					<button
						type="button"
						className="primary"
						disabled={working}
						onClick={onConfirm}
					>
						{remove ? "Remove Knock" : "Confirm and connect"}
					</button>
				)}
				<button type="button" onClick={onCancel}>
					{preview.ok ? "Cancel" : "Close"}
				</button>
			</div>
		</fieldset>
	);
}

/** What happened, and what to do next. */
function Done({
	step,
	onClose,
}: {
	step: Extract<Step, { kind: "done" }>;
	onClose(): void;
}) {
	const { agent, remove, result } = step;
	return (
		<div className="offer" role="status">
			{result.ok ? (
				<>
					<p>
						<strong>
							{remove
								? `Knock is removed from ${agent.name}.`
								: `Knock is connected to ${agent.name}.`}
						</strong>
					</p>
					{!remove && result.note && <p>{result.note}</p>}
					<Backup path={result.backup} />
				</>
			) : (
				<p role="alert">{result.message}</p>
			)}
			<div className="actions start">
				<button type="button" onClick={onClose}>
					Close
				</button>
			</div>
		</div>
	);
}

/**
 * Settings' "Other agents" section: who's connected, and the way to the Agents page,
 * so the main page stays short.
 */
export function AgentsSummary({ onOpen }: { onOpen(): void }) {
	const id = useId();
	const [agents, setAgents] = useState<AgentStatus[] | null>(null);
	useEffect(() => {
		void invoke<AgentStatus[]>("agents_status").then(setAgents, () => {});
	}, []);
	const connected = (agents ?? []).filter((a) => a.state === "installed");
	return (
		<section aria-labelledby={`${id}t`}>
			<h2 id={`${id}t`}>Other agents</h2>
			<p className="help">
				{connected.length > 0
					? `Knock also watches ${connected.map((a) => a.name).join(", ")}.`
					: "Knock watches Claude Code. Antigravity works in part (beta); Codex and GitHub Copilot aren't available yet."}
			</p>
			<div className="actions start">
				<button type="button" onClick={onOpen}>
					Connect other agents…
				</button>
			</div>
		</section>
	);
}
