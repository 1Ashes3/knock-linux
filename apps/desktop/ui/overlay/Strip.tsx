import {
	type CSSProperties,
	type ReactNode,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { useStore } from "zustand";
import { call, type HitRegion, type SessionView } from "./bridge";
import {
	accessibleName,
	dismissible,
	type Glyph,
	glyph,
	INK,
	knocks,
	layout,
	OVERFLOW_FILL,
	overflowName,
	RED,
	SLOT_PX,
	shortWord,
	TIER_FILL,
} from "./pill";
import type { StripStore } from "./store";

export const STALL_DEFAULT = 10; // §8.6 default, until settings arrive.

/** Calls `cmd` only when its args differ from the previous call. */
export function useSend(cmd: string) {
	const last = useRef("");
	return (args: Record<string, unknown>) => {
		const key = JSON.stringify(args);
		if (key === last.current) return;
		last.current = key;
		void call(cmd, args);
	};
}

/** Re-renders once a minute while `on`, so "No progress for <n> min" stays current. */
export function useMinuteClock(on: boolean): number {
	const [now, setNow] = useState(Date.now);
	useEffect(() => {
		if (!on) return;
		setNow(Date.now());
		const t = setInterval(() => setNow(Date.now()), 60_000);
		return () => clearInterval(t);
	}, [on]);
	return now;
}

export function Strip({ store }: { store: StripStore }) {
	const sessions = useStore(store, (s) => s.sessions);
	const size = useStore(store, (s) => s.settings?.size ?? "medium");
	const stallMinutes = useStore(
		store,
		(s) => s.settings?.stallMinutes ?? STALL_DEFAULT,
	);
	const now = useMinuteClock(sessions.some((s) => s.state === "stalled"));
	const ref = useRef<HTMLDivElement>(null);
	const sendSize = useSend("request_strip_size");
	const sendRegions = useSend("set_hit_regions");

	const h = SLOT_PX[size];
	const { visible, hidden } = layout(sessions);
	const width = (visible.length + (hidden.length ? 1 : 0)) * h;

	useEffect(() => sendSize({ width, height: h }));

	useLayoutEffect(() => {
		const report = () => {
			const regions: HitRegion[] = [];
			for (const el of ref.current?.querySelectorAll("[data-hit]") ?? []) {
				const { x, y, width, height } = el.getBoundingClientRect();
				regions.push({ x, y, width, height });
			}
			sendRegions({ regions });
		};
		report();
		window.addEventListener("resize", report);
		return () => window.removeEventListener("resize", report);
	});

	return (
		<div
			ref={ref}
			className="strip"
			role="group"
			aria-label="Claude Code sessions"
			style={{ "--s": `${h}px`, width, height: h } as CSSProperties}
		>
			{visible.map((s) => (
				<Pill key={s.id} s={s} name={accessibleName(s, now, stallMinutes)} />
			))}
			{hidden.length > 0 && <Overflow hidden={hidden} />}
		</div>
	);
}

function Pill({ s, name }: { s: SessionView; name: string }) {
	return (
		<div className="slot">
			<div
				className={knocks(s) ? "pill knock" : "pill"}
				role="button"
				aria-label={name}
				data-hit=""
				data-state={s.state}
				style={{ "--fill": TIER_FILL[s.tier] } as CSSProperties}
				onClick={() => void call("jump_to_session", { id: s.id })}
			>
				<GlyphIcon g={glyph(s)} />
				<span className="word">{shortWord(s)}</span>
				<span className="project">{s.project}</span>
			</div>
			{dismissible(s) && (
				<div
					className="dismiss"
					role="button"
					aria-label={`Dismiss ${s.project}`}
					data-hit=""
					onClick={() => void call("dismiss", { id: s.id })}
				>
					<svg viewBox="0 0 12 12" aria-hidden="true">
						<circle cx="6" cy="6" r="5.4" fill={INK} stroke="#fff" />
						<path
							d="M4 4L8 8M8 4L4 8"
							stroke="#fff"
							strokeWidth="1.5"
							strokeLinecap="round"
						/>
					</svg>
				</div>
			)}
		</div>
	);
}

/** "+N": sessions beyond the 5 pills. Not clickable yet (the den is Phase 4), so no hit region. */
export function Overflow({ hidden }: { hidden: SessionView[] }) {
	const urgent = hidden.some((s) => s.tier === "needs_you");
	return (
		<div className="slot">
			<div
				className="pill overflow"
				role="img"
				aria-label={overflowName(hidden)}
				style={
					{
						"--fill": urgent ? TIER_FILL.needs_you : OVERFLOW_FILL,
					} as CSSProperties
				}
			>
				{urgent && <GlyphIcon g="ask" />}
				<span className="word">+{hidden.length}</span>
				<span className="project">more</span>
			</div>
		</div>
	);
}

const MARKS: Record<Glyph, ReactNode> = {
	ask: (
		<>
			<path d="M4.3 4.6C4.3 3.5 5.1 2.8 6.1 2.8C7.1 2.8 7.8 3.5 7.8 4.4C7.8 5.6 6.1 5.8 6.1 7" />
			<circle cx="6.1" cy="9.2" r="0.5" fill="#fff" />
		</>
	),
	failed: (
		<>
			<path d="M6 3V6.6" />
			<circle cx="6" cy="9" r="0.6" fill="#fff" />
		</>
	),
	check: <path d="M3.4 6.3L5.2 8.1L8.7 4.3" />,
	dots: (
		<>
			<circle cx="3.4" cy="6" r="0.4" fill="#fff" />
			<circle cx="6" cy="6" r="0.4" fill="#fff" />
			<circle cx="8.6" cy="6" r="0.4" fill="#fff" />
		</>
	),
	z: <path d="M4 4H8L4 8H8" />,
};

export function GlyphIcon({ g }: { g: Glyph }) {
	return (
		<svg className="glyph" viewBox="0 0 12 12" aria-hidden="true">
			<circle cx="6" cy="6" r="6" fill={g === "failed" ? RED : INK} />
			<g
				fill="none"
				stroke="#fff"
				strokeWidth="1.6"
				strokeLinecap="round"
				strokeLinejoin="round"
			>
				{MARKS[g]}
			</g>
		</svg>
	);
}
