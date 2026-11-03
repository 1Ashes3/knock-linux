// Animals mode (SPEC.md §10.1, §10.3, §10.5, FR-3, FR-4, FR-7). The canvas draws the
// animals; everything else (chips, tooltip, badges, the red marker, the "z", buttons and
// the names Narrator reads) is plain DOM on top, so text stays crisp and accessible.
import {
	type CSSProperties,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { useStore } from "zustand";
import { ZZZ_LETTERS } from "../../gallery/zzz.ts";
import { call, type HitRegion } from "../bridge";
import { accessibleName, dismissible, glyph, INK, TIER_FILL } from "../pill";
import {
	GlyphIcon,
	Overflow,
	STALL_DEFAULT,
	useMinuteClock,
	useSend,
} from "../Strip";
import type { StripStore } from "../store";
import { CLIPS } from "./art";
import { endsRaised, frameAt } from "./controller";
import {
	CHIP,
	type ChipInput,
	characterRect,
	chipText,
	dismissRect,
	hitRegions,
	layoutChips,
	type Metrics,
	metrics,
	slotTop,
	slotX,
	stripWidth,
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
}: {
	store: StripStore;
	/** The renderer could not start: the overlay shows the plain strip instead. */
	onUnavailable: () => void;
}) {
	const sessions = useStore(store, (s) => s.sessions);
	const size = useStore(store, (s) => s.settings?.size ?? "medium");
	const stallMinutes = useStore(
		store,
		(s) => s.settings?.stallMinutes ?? STALL_DEFAULT,
	);
	// §6, §10.1: Windows "Animation effects" off.
	const still = useMedia("(prefers-reduced-motion: reduce)");
	const dpr = useDevicePixelRatio();
	const [hover, setHover] = useState<string | null>(null);
	const [woken, wake] = useState(0);
	useMinuteClock(hover !== null || sessions.some((s) => s.state === "stalled"));

	const memory = useRef(EMPTY_MEMORY);
	const [measureFor] = useState(createMeasure);
	const now = Date.now();
	const { model, memory: next } = step(
		sessions,
		memory.current,
		still,
		now,
		CLIPS,
	);
	memory.current = next;

	const m = metrics(size);
	const width = stripWidth(model.slots, m);
	const chipFont = measureFor(`600 ${m.font}px ${FONT}`);
	const shown = model.characters.filter(
		(c) => !c.leaving && (c.fadeFrom === null || now < c.fadeFrom),
	);
	const chipInputs: ChipInput[] = [];
	for (const c of shown) {
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
	const height = model.slots ? chipArea + m.headroom + m.s : m.s;
	const raised = (c: Character) => !still && endsRaised(c.plan);

	// §9.2: the core places the window; it grows up and left from the corner.
	const sendSize = useSend("request_strip_size");
	useEffect(() => sendSize({ width, height }));

	// §9.3: only characters and buttons catch the mouse. Sent only when they change.
	const ref = useRef<HTMLDivElement>(null);
	const sendRegions = useSend("set_hit_regions");
	const local = hitRegions(
		shown.map((c) => ({
			slot: c.slot,
			raised: raised(c),
			dismiss: dismissible(c.session),
		})),
		chipArea,
		m,
	);
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
	// Re-armed after every wake, in case a timer fires a millisecond early.
	// biome-ignore lint/correctness/useExhaustiveDependencies: `woken` re-arms the timer
	useEffect(() => {
		if (!Number.isFinite(model.wakeAt)) return;
		const t = setTimeout(
			() => wake((n) => n + 1),
			Math.max(0, model.wakeAt - Date.now()) + 1,
		);
		return () => clearTimeout(t);
	}, [model.wakeAt, woken]);

	// The canvas covers the headroom and the slots.
	const host = useRef<HTMLDivElement>(null);
	const renderer = useRef<AnimalRenderer | null>(null);
	const view = { width, height: m.headroom + m.s, s: m.s, dpr };
	const draw: DrawCharacter[] = model.characters.map((c) => ({
		id: c.session.id,
		animal: c.session.animal,
		x: slotX(c.slot, m),
		y: m.headroom,
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

	const owner = (id: string) => shown.find((c) => c.session.id === id);
	const hovered = owner(hover ?? "");
	// A character that goes away under the mouse gets no mouseleave.
	useEffect(() => {
		if (hover !== null && !hovered) setHover(null);
	});
	return (
		<div
			ref={ref}
			className="animals"
			role="group"
			aria-label="Claude Code sessions"
			style={{ "--s": `${m.s}px`, width, height } as CSSProperties}
		>
			<div
				ref={host}
				className="stage"
				style={{ top: chipArea, width, height: m.headroom + m.s }}
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
					settled={frameAt(c.plan, now, CLIPS).settled}
					name={accessibleName(c.session, now, stallMinutes)}
					onHover={setHover}
				/>
			))}
			{shown
				.filter((c) => dismissible(c.session))
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
					);
				})}
			{model.hidden.length > 0 && (
				<div
					className="overflow-slot"
					style={{ left: slotX(model.slots - 1, m), top: slotTop(chipArea, m) }}
				>
					<Overflow hidden={model.hidden} />
				</div>
			)}
			{hovered && (
				<Tooltip
					c={hovered}
					now={now}
					stallMinutes={stallMinutes}
					bottom={characterRect(hovered.slot, raised(hovered), chipArea, m).y}
					width={width}
					m={m}
					measureFor={measureFor}
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
}: {
	c: Character;
	rect: HitRegion;
	m: Metrics;
	still: boolean;
	settled: boolean;
	name: string;
	onHover: (id: string | null) => void;
}) {
	const s = c.session;
	const id = s.id;
	return (
		<div
			className="character"
			role="button"
			aria-label={name}
			data-state={s.state}
			style={{
				left: rect.x,
				top: rect.y,
				width: rect.width,
				height: rect.height,
			}}
			onClick={() => void call("jump_to_session", { id })}
			onMouseEnter={() => onHover(id)}
			onMouseLeave={() => onHover(null)}
		>
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
		</div>
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

/** FR-7: project, state label, time in state and, for Needs you, the decision type. */
function Tooltip({
	c,
	now,
	stallMinutes,
	bottom,
	width,
	m,
	measureFor,
}: {
	c: Character;
	now: number;
	stallMinutes: number;
	bottom: number;
	width: number;
	m: Metrics;
	measureFor: (font: string) => (text: string) => number;
}) {
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
	return (
		<div
			className="tooltip"
			aria-hidden="true"
			style={{
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
