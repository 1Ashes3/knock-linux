// Animals mode (SPEC.md §10.1, §10.3, §10.5, FR-3, FR-4, FR-7). The canvas draws the
// animals; everything else (chips, tooltip, badges, the red marker, the "z", buttons and
// the names Narrator reads) is plain DOM on top, so text stays crisp and accessible.
import {
	type CSSProperties,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import { useStore } from "zustand";
import { ZZZ_LETTERS } from "../../gallery/zzz.ts";
import { AwayAnnouncer, AwayCard, cardLayout } from "../AwayCard";
import { cardOffset, withCard } from "../away";
import { call, type HitRegion } from "../bridge";
import { Den } from "../Den";
import { CloseIcon, GlyphIcon } from "../Glyph";
import { demoAt } from "../introDemo";
import {
	accessibleName,
	dismissible,
	glyph,
	INK,
	MAX_VISIBLE,
	TIER_FILL,
} from "../pill";
import {
	POINTER_ENTERED,
	POINTER_LEFT,
	useVisibleSessions,
} from "../placement";
import { STALL_DEFAULT, useMinuteClock, useSend } from "../Strip";
import { tagHidden } from "../stallTag";
import type { StripStore } from "../store";
import { clipsFor, packVersion, subscribePack } from "./art";
import { endsRaised, frameAt } from "./controller";
import {
	CHIP,
	type ChipInput,
	characterRect,
	chipText,
	dismissRect,
	hitRegions,
	introMetrics,
	layoutChips,
	type Metrics,
	metrics,
	rowHeight,
	slotTop,
	slotX,
	stripWidth,
	tagDismissRect,
	tooltipBox,
	tooltipLines,
	tooltipReserve,
} from "./layout";
import { type Character, EMPTY_MEMORY, step } from "./model";
import { AnimalRenderer, type DrawCharacter } from "./renderer";
import "../../gallery/zzz.css";
import "./animals.css";

const FONT = '"Segoe UI", system-ui, sans-serif';

/** Text widths from the same font the DOM uses. */
function createMeasure(): (font: string) => (text: string) => number {
	const ctx = document.createElement("canvas").getContext("2d");
	return (font) => (text) => {
		if (!ctx) return text.length * 7;
		ctx.font = font;
		return ctx.measureText(text).width + 1; // a pixel of slack for rounding
	};
}

function useMedia(query: string): boolean {
	const [on, setOn] = useState(() => matchMedia(query).matches);
	useEffect(() => {
		const mq = matchMedia(query);
		const change = () => setOn(mq.matches);
		change();
		mq.addEventListener("change", change);
		return () => mq.removeEventListener("change", change);
	}, [query]);
	return on;
}

/** Follows the screen density, for crisp animals at 100%, 150% and 200% (§6). */
function useDevicePixelRatio(): number {
	const [dpr, setDpr] = useState(() => devicePixelRatio);
	useEffect(() => {
		const mq = matchMedia(`(resolution: ${dpr}dppx)`);
		const change = () => setDpr(devicePixelRatio);
		mq.addEventListener("change", change);
		return () => mq.removeEventListener("change", change);
	}, [dpr]);
	return dpr;
}

export function AnimalStrip({
	store,
	onUnavailable,
	onDismissAway,
}: {
	store: StripStore;
	/** The renderer could not start: the overlay shows the plain strip instead. */
	onUnavailable: () => void;
	/** A click on the away card (FR-9). */
	onDismissAway: () => void;
}) {
	const all = useStore(store, (s) => s.sessions);
	const placement = useStore(store, (s) => s.settings?.placement);
	const real = useVisibleSessions(all, placement);
	// ADR 0040: "Introduce sprites" draws its own made-up sprites instead of the real ones,
	// which stay in the store untouched. They take no clicks and send nothing to the core.
	const demo = useStore(store, (s) => s.demo);
	const frame = demo && demoAt(demo, Date.now());
	const demoOn = frame !== null && frame.phase !== "over";
	const sessions = demoOn ? frame.sessions : real;
	const away = useStore(store, (s) => s.away);
	const hiddenTags = useStore(store, (s) => s.hiddenTags);
	const size = useStore(store, (s) => s.settings?.size ?? "medium");
	const idleMotion = useStore(store, (s) => s.settings?.idleMotion ?? true);
	const stallMinutes = useStore(
		store,
		(s) => s.settings?.stallMinutes ?? STALL_DEFAULT,
	);
	// §6, §10.1: Windows "Animation effects" off.
	const still = useMedia("(prefers-reduced-motion: reduce)");
	const dpr = useDevicePixelRatio();
	// The card stays while the mouse is on the character or the card itself, then fades
	// out over `cardFadeSeconds`, like a notification; coming back cancels the fade.
	const fadeMs = useStore(
		store,
		(s) => (s.settings?.cardFadeSeconds ?? CARD_FADE_DEFAULT) * 1000,
	);
	const fadeMsRef = useRef(fadeMs);
	fadeMsRef.current = fadeMs;
	const [hover, setHover] = useState<string | null>(null);
	const [fading, setFading] = useState(false);
	// Linux: hovers count only while the pointer is really in the strip. WebKitGTK replays
	// the last position as a fake move when the page changes, after the pointer has gone.
	// Windows sends neither event, so it stays true there.
	const inside = useRef(true);
	const fadeTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const enter = (id: string) => {
		if (!inside.current) return;
		clearTimeout(fadeTimer.current);
		setFading(false);
		setHover(id);
	};
	const leave = useRef(() => {
		clearTimeout(fadeTimer.current);
		setFading(true);
		fadeTimer.current = setTimeout(() => {
			setHover(null);
			setFading(false);
		}, fadeMsRef.current);
	}).current;
	useEffect(() => {
		const left = () => {
			inside.current = false;
			leave();
		};
		const entered = () => {
			inside.current = true;
		};
		window.addEventListener(POINTER_LEFT, left);
		window.addEventListener(POINTER_ENTERED, entered);
		return () => {
			window.removeEventListener(POINTER_LEFT, left);
			window.removeEventListener(POINTER_ENTERED, entered);
			clearTimeout(fadeTimer.current);
		};
	}, [leave]);
	const onHover = (id: string | null) => (id === null ? leave() : enter(id));
	const [woken, wake] = useState(0);
	useMinuteClock(hover !== null || sessions.some((s) => s.state === "stalled"));

	// A new sprite pack (ADR 0038) draws the strip again.
	useSyncExternalStore(subscribePack, packVersion);
	const memory = useRef(EMPTY_MEMORY);
	// The demo's own memory, fresh for each of its phases: the real characters keep
	// theirs, so they come back as they were, and a demo sprite never fades into them.
	const demoMemory = useRef({ key: "", memory: EMPTY_MEMORY });
	const [measureFor] = useState(createMeasure);
	const now = Date.now();
	const demoKey = demoOn ? `${demo?.startedAt}:${frame.phase}` : "";
	if (demoKey !== demoMemory.current.key)
		demoMemory.current = { key: demoKey, memory: EMPTY_MEMORY };
	const { model, memory: next } = step(
		sessions,
		demoOn ? demoMemory.current.memory : memory.current,
		still,
		now,
		clipsFor,
		idleMotion,
		demoOn ? Number.POSITIVE_INFINITY : MAX_VISIBLE,
	);
	if (demoOn) demoMemory.current.memory = next;
	else memory.current = next;

	const m = demoOn
		? introMetrics(size, model.characters.length)
		: metrics(size);
	const width = stripWidth(model.slots, m);
	const chipFont = measureFor(`600 ${m.font}px ${FONT}`);
	const shown = model.characters.filter(
		(c) => !c.leaving && (c.fadeFrom === null || now < c.fadeFrom),
	);
	const chipInputs: ChipInput[] = [];
	for (const c of shown) {
		// The user closed this stall's "No progress" tag: the character stays, the chip goes.
		if (tagHidden(hiddenTags, c.session)) continue;
		const text = chipText(c.session, now, stallMinutes);
		if (text)
			chipInputs.push({
				id: c.session.id,
				anchor: slotX(c.slot, m) + m.s / 2,
				text,
			});
	}
	const chips = layoutChips(chipInputs, width, chipFont, m);
	const chipArea = Math.max(chips.height, tooltipReserve(m));
	const chipShift = chipArea - chips.height;
	const stageHeight = m.rows * rowHeight(m);
	const height = model.slots ? chipArea + stageHeight : m.s;
	const raised = (c: Character) => !still && endsRaised(c.plan);
	const owner = (id: string) => shown.find((c) => c.session.id === id);
	// A stalled session's chip gets an "x" that hides only the chip.
	const tagButtons = chips.chips.flatMap((chip) => {
		const c = owner(chip.id);
		return !demoOn && c?.session.state === "stalled"
			? [
					{
						session: c.session,
						rect: tagDismissRect(chip, chipShift, width, m),
					},
				]
			: [];
	});

	// FR-9: the away card stacks above everything (chips, the tooltip's room, the
	// characters), right edges level, so it covers nothing and the window only grows up.
	// Relative to the strip's top-left.
	const card = cardLayout(away, size);
	const cardRect: HitRegion | null = card && {
		...cardOffset({ width, height }, card),
		width: card.width,
		height: card.height,
	};

	// §9.2: the core places the window; it grows up and left from the corner.
	const sendSize = useSend("request_strip_size");
	// The window's cap is 6 usual slots, also while the demo shrinks its characters.
	const slotPx = metrics(size).s;
	useEffect(() => sendSize(withCard({ width, height }, card, slotPx)));

	// §9.3: only characters, buttons and the away card catch the mouse. Sent only when
	// they change.
	const ref = useRef<HTMLDivElement>(null);
	const sendRegions = useSend("set_hit_regions");
	// Demo sprites take no clicks: the mouse goes through them.
	const local = demoOn
		? []
		: hitRegions(
				shown.map((c) => ({
					slot: c.slot,
					raised: raised(c),
					dismiss: dismissible(c.session),
				})),
				chipArea,
				m,
			);
	for (const b of tagButtons) local.push(b.rect);
	if (cardRect) local.push(cardRect);
	const hovered = demoOn ? undefined : owner(hover ?? "");
	const tip =
		hovered &&
		tooltipLayout(
			hovered,
			now,
			stallMinutes,
			characterRect(hovered.slot, raised(hovered), chipArea, m).y,
			width,
			m,
			measureFor,
		);
	// The card catches the mouse too, so it can be read with the pointer on it.
	if (tip) local.push(tip.box);
	useLayoutEffect(() => {
		const report = () => {
			const box = ref.current?.getBoundingClientRect();
			const regions: HitRegion[] = local.map((r) => ({
				...r,
				x: r.x + (box?.x ?? 0),
				y: r.y + (box?.y ?? 0),
			}));
			sendRegions({ regions });
		};
		report();
		window.addEventListener("resize", report);
		return () => window.removeEventListener("resize", report);
	});

	// Wake up when something changes on its own: a calm animal settles, an ended one fades.
	// Re-armed after every wake, in case a timer fires a millisecond early. The demo's
	// next step wakes it too.
	const wakeAt = Math.min(
		model.wakeAt,
		demoOn ? frame.nextAt : Number.POSITIVE_INFINITY,
	);
	// biome-ignore lint/correctness/useExhaustiveDependencies: `woken` re-arms the timer
	useEffect(() => {
		if (!Number.isFinite(wakeAt)) return;
		const t = setTimeout(
			() => wake((n) => n + 1),
			Math.max(0, wakeAt - Date.now()) + 1,
		);
		return () => clearTimeout(t);
	}, [wakeAt, woken]);

	// The canvas covers the headroom and the slots.
	const host = useRef<HTMLDivElement>(null);
	const renderer = useRef<AnimalRenderer | null>(null);
	const view = { width, height: stageHeight, s: m.s, dpr };
	const draw: DrawCharacter[] = model.characters.map((c) => ({
		id: c.session.id,
		animal: c.session.animal,
		x: slotX(c.slot, m),
		y: slotTop(0, m, c.slot),
		plan: c.plan,
		fadeFrom: c.fadeFrom,
	}));
	const latest = useRef({ draw, view });
	latest.current = { draw, view };
	// biome-ignore lint/correctness/useExhaustiveDependencies: created once; later changes go through sync
	useEffect(() => {
		if (!host.current) return;
		const r = new AnimalRenderer(host.current);
		renderer.current = r;
		r.init(latest.current.view).then(
			() => r.sync(latest.current.draw, latest.current.view),
			(e) => {
				console.warn("Animals are unavailable; showing plain labels.", e);
				onUnavailable();
			},
		);
		return () => {
			r.destroy();
			renderer.current = null;
		};
	}, []);
	// In the commit itself, so the canvas changes in the same frame as the DOM.
	useLayoutEffect(() => {
		renderer.current?.sync(draw, view);
	});

	// A character that goes away under the mouse gets no mouseleave.
	useEffect(() => {
		if (hover !== null && !hovered) setHover(null);
	});
	return (
		<div
			ref={ref}
			className="animals"
			role="group"
			aria-label={demoOn ? "Introducing sprites" : "Claude Code sessions"}
			style={{ "--s": `${m.s}px`, width, height } as CSSProperties}
		>
			<div
				ref={host}
				className="stage"
				style={{ top: chipArea, width, height: stageHeight }}
			/>
			<Leaders
				chips={chips.chips}
				shift={chipShift}
				width={width}
				height={slotTop(chipArea, m)}
				owner={owner}
				ends={(c) => characterRect(c.slot, raised(c), chipArea, m).y}
			/>
			{chips.chips.map((c) => {
				const s = owner(c.id)?.session;
				return (
					<div
						key={c.id}
						className="chip"
						aria-hidden="true"
						data-chip={c.id}
						style={
							{
								left: c.x,
								top: c.y + chipShift,
								width: c.width,
								height: c.height,
								fontSize: m.font,
								lineHeight: `${m.lineHeight}px`,
								"--fill": s ? TIER_FILL[s.tier] : "#fff",
							} as CSSProperties
						}
					>
						{c.lines.map((line, i) => (
							// biome-ignore lint/suspicious/noArrayIndexKey: lines have no identity
							<span key={i}>{line}</span>
						))}
					</div>
				);
			})}
			{shown.map((c) => (
				<CharacterButton
					key={c.session.id}
					c={c}
					rect={characterRect(c.slot, raised(c), chipArea, m)}
					m={m}
					still={still}
					settled={frameAt(c.plan, now, clipsFor(c.session.animal)).settled}
					name={accessibleName(c.session, now, stallMinutes)}
					onHover={onHover}
					demo={demoOn}
				/>
			))}
			{shown
				.filter((c) => !demoOn && dismissible(c.session))
				.map((c) => {
					const r = dismissRect(c.slot, chipArea, m);
					return (
						<div
							key={`dismiss-${c.session.id}`}
							className="dismiss"
							role="button"
							aria-label={`Dismiss ${c.session.project}`}
							style={{ left: r.x, top: r.y, width: r.width, height: r.height }}
							onClick={() => void call("dismiss", { id: c.session.id })}
						>
							<CloseIcon />
						</div>
					);
				})}
			{tagButtons.map(({ session, rect: r }) => (
				<div
					key={`hide-tag-${session.id}`}
					className="dismiss"
					role="button"
					aria-label={`Hide the no-progress notice for ${session.project}`}
					data-tag-dismiss={session.id}
					style={{ left: r.x, top: r.y, width: r.width, height: r.height }}
					onClick={() => store.getState().hideStallTag(session.id)}
				>
					<CloseIcon />
				</div>
			))}
			{model.hidden.length > 0 && (
				<div
					className="den-slot"
					style={{
						left: slotX(model.slots - 1, m),
						top: slotTop(chipArea, m, model.slots - 1),
						width: m.s,
						height: m.s,
					}}
				>
					<Den hidden={model.hidden} look="mound" />
				</div>
			)}
			{card && cardRect && (
				<AwayCard
					card={card}
					onDismiss={onDismissAway}
					style={{ left: cardRect.x, top: cardRect.y }}
				/>
			)}
			<AwayAnnouncer card={card} />
			{hovered && tip && (
				<Tooltip
					tip={tip}
					m={m}
					fading={fading}
					fadeMs={fadeMs}
					still={still}
					onEnter={() => enter(hovered.session.id)}
					onLeave={leave}
				/>
			)}
		</div>
	);
}

/** One character's button: the hit region, Narrator's name, and the shared marks on it. */
function CharacterButton({
	c,
	rect,
	m,
	still,
	settled,
	name,
	onHover,
	demo,
}: {
	c: Character;
	rect: HitRegion;
	m: Metrics;
	still: boolean;
	settled: boolean;
	name: string;
	onHover: (id: string | null) => void;
	/** A sprite of "Introduce sprites" (ADR 0040): named, but not a button. */
	demo: boolean;
}) {
	const s = c.session;
	const id = s.id;
	const style = {
		left: rect.x,
		top: rect.y,
		width: rect.width,
		height: rect.height,
	};
	const marks = <Marks s={s} m={m} still={still} settled={settled} />;
	if (demo)
		return (
			<div
				className="character demo"
				role="img"
				aria-label={name}
				data-state={s.state}
				style={style}
			>
				{marks}
			</div>
		);
	return (
		<div
			className="character"
			role="button"
			aria-label={name}
			data-state={s.state}
			style={style}
			onClick={() => void call("jump_to_session", { id })}
			onMouseEnter={() => onHover(id)}
			// A real move after the pointer came back: the enter may have been ignored
			// while the core's "pointer-entered" was still on its way.
			onMouseMove={() => onHover(id)}
			onMouseLeave={() => onHover(null)}
		>
			{marks}
		</div>
	);
}

/** The shared marks on a character: the "z", the red failure marker, the badges. */
function Marks({
	s,
	m,
	still,
	settled,
}: {
	s: Character["session"];
	m: Metrics;
	still: boolean;
	settled: boolean;
}) {
	return (
		<>
			{s.state === "idle" && (
				<svg className="zzz" viewBox="0 0 64 64" aria-hidden="true">
					<g className={settled || still ? "knock-zzz settled" : "knock-zzz"}>
						{ZZZ_LETTERS.map(([letter, x, y, size]) => (
							<text key={`${x}`} x={x} y={y} fontSize={size}>
								{letter}
							</text>
						))}
					</g>
				</svg>
			)}
			{s.state === "failed" ? (
				<div className="marker" aria-hidden="true" style={markSize(m)}>
					<GlyphIcon g="failed" />
				</div>
			) : (
				still && (
					<div
						className="badge"
						aria-hidden="true"
						data-badge={glyph(s)}
						style={markSize(m)}
					>
						<GlyphIcon g={glyph(s)} />
					</div>
				)
			)}
		</>
	);
}

const markSize = (m: Metrics): CSSProperties => {
	const d = Math.round(m.s * 0.28);
	return { width: d, height: d };
};

/**
 * A tail under every chip, and a leader from it down to the top of its own character.
 * Drawn in the chip's fill with a dark outline, like the chip, so they read on any background.
 */
function Leaders({
	chips,
	shift,
	width,
	height,
	owner,
	ends,
}: {
	chips: ReturnType<typeof layoutChips>["chips"];
	shift: number;
	width: number;
	height: number;
	owner: (id: string) => Character | undefined;
	/** Where a leader ends: the top of its character. */
	ends: (c: Character) => number;
}) {
	if (!chips.length) return null;
	return (
		<svg className="leaders" width={width} height={height} aria-hidden="true">
			{chips.map((chip) => {
				const c = owner(chip.id);
				if (!c) return null;
				const fill = TIER_FILL[c.session.tier];
				const top = chip.y + shift + chip.height - CHIP.border;
				const tip = top + CHIP.tail;
				const end = ends(c) - 1;
				const half = CHIP.tail * 0.9;
				const x = chip.anchor;
				return (
					<g key={chip.id} strokeLinecap="round" strokeLinejoin="round">
						{tip < end && (
							<>
								<line
									x1={x}
									y1={top}
									x2={x}
									y2={end}
									stroke={INK}
									strokeWidth={4}
								/>
								<line
									x1={x}
									y1={top}
									x2={x}
									y2={end - 1}
									stroke={fill}
									strokeWidth={1.5}
								/>
							</>
						)}
						<path
							d={`M${x - half} ${top}L${x} ${tip}L${x + half} ${top}`}
							fill={fill}
							stroke={INK}
							strokeWidth={CHIP.border}
						/>
					</g>
				);
			})}
		</svg>
	);
}

/** `cardFadeSeconds` until settings arrive (ADR 0037). */
const CARD_FADE_DEFAULT = 3;

/** FR-7: the card's lines and where it goes, above the character's top at `bottom`. */
function tooltipLayout(
	c: Character,
	now: number,
	stallMinutes: number,
	bottom: number,
	width: number,
	m: Metrics,
	measureFor: (font: string) => (text: string) => number,
) {
	const lines = tooltipLines(c.session, now, stallMinutes);
	const bold = measureFor(`700 ${m.font}px ${FONT}`);
	const regular = measureFor(`400 ${m.font}px ${FONT}`);
	const box = tooltipBox(
		lines.map((l, i) => (i === 0 ? bold(l) : regular(l))),
		slotX(c.slot, m) + m.s / 2,
		bottom - 3,
		width,
		m,
	);
	return { lines, box };
}

/** FR-7: project, state label, time in state and, for Needs you, the decision type. */
function Tooltip({
	tip: { lines, box },
	m,
	fading,
	fadeMs,
	still,
	onEnter,
	onLeave,
}: {
	tip: ReturnType<typeof tooltipLayout>;
	m: Metrics;
	fading: boolean;
	fadeMs: number;
	still: boolean;
	onEnter: () => void;
	onLeave: () => void;
}) {
	return (
		<div
			className="tooltip"
			aria-hidden="true"
			data-fading={fading && !still ? "" : undefined}
			onMouseEnter={onEnter}
			onMouseLeave={onLeave}
			style={{
				// Fades out slowly; comes back at once.
				transitionDuration: fading ? `${fadeMs}ms` : "0ms",
				left: box.x,
				top: box.y,
				width: box.width,
				height: box.height,
				fontSize: m.font,
				lineHeight: `${m.lineHeight}px`,
			}}
		>
			{lines.map((line, i) => (
				// biome-ignore lint/suspicious/noArrayIndexKey: lines have no identity
				<span key={i} className={i === 0 ? "project" : undefined}>
					{line}
				</span>
			))}
		</div>
	);
}
