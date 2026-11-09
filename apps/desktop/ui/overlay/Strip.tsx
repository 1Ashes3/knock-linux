import {
	type CSSProperties,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { useStore } from "zustand";
import { AwayAnnouncer, AwayCard, cardLayout } from "./AwayCard";
import { cardOffset, withCard } from "./away";
import { call, type HitRegion, type SessionView } from "./bridge";
import { Den } from "./Den";
import { CloseIcon, GlyphIcon } from "./Glyph";
import {
	accessibleName,
	dismissible,
	glyph,
	knocks,
	layout,
	SLOT_PX,
	shortWord,
	TIER_FILL,
} from "./pill";
import { visibleSessions } from "./placement";
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

export function Strip({
	store,
	onDismissAway,
}: {
	store: StripStore;
	/** A click on the away card (FR-9). */
	onDismissAway: () => void;
}) {
	const all = useStore(store, (s) => s.sessions);
	const placement = useStore(store, (s) => s.settings?.placement);
	const sessions = useMemo(
		() => visibleSessions(all, placement),
		[all, placement],
	);
	const size = useStore(store, (s) => s.settings?.size ?? "medium");
	const stallMinutes = useStore(
		store,
		(s) => s.settings?.stallMinutes ?? STALL_DEFAULT,
	);
	const away = useStore(store, (s) => s.away);
	const now = useMinuteClock(sessions.some((s) => s.state === "stalled"));
	const ref = useRef<HTMLDivElement>(null);
	const sendSize = useSend("request_strip_size");
	const sendRegions = useSend("set_hit_regions");

	const h = SLOT_PX[size];
	const { visible, hidden } = layout(sessions);
	const width = (visible.length + (hidden.length ? 1 : 0)) * h;
	const card = cardLayout(away, size);

	useEffect(() => sendSize(withCard({ width, height: h }, card, h)));
	// FR-9: the card stacks above the pills, right edges level (§9.2: never wider).
	const cardAt = card && cardOffset({ width, height: h }, card);

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
		<div ref={ref}>
			<div
				className="strip"
				role="group"
				aria-label="Claude Code sessions"
				style={{ "--s": `${h}px`, width, height: h } as CSSProperties}
			>
				{visible.map((s) => (
					<Pill key={s.id} s={s} name={accessibleName(s, now, stallMinutes)} />
				))}
				{hidden.length > 0 && (
					<div className="slot">
						<Den hidden={hidden} look="pill" />
					</div>
				)}
			</div>
			{card && cardAt && (
				<AwayCard
					card={card}
					onDismiss={onDismissAway}
					style={{
						right: "var(--strip-right, 0px)",
						bottom: `calc(var(--strip-bottom, 0px) + ${h - cardAt.y - card.height}px)`,
					}}
				/>
			)}
			<AwayAnnouncer card={card} />
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
					<CloseIcon />
				</div>
			)}
		</div>
	);
}
