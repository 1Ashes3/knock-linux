// The art the strip draws (SPEC.md §8.7, ADR 0038): Knock's own animals and clips, bundled
// at build time (the "classic" pack), and the active sprite pack, which the core checked and
// serves from memory. Part SVGs are separate files (`no-inline`), because the app's CSP allows
// images only from the app itself and from `knockpack:`, not data: or blob: URLs.
import type { Clip, Rig } from "../../gallery/format.ts";
import type { PackData } from "../bridge";
import type { ClipName } from "./controller";

const rigFiles = import.meta.glob<Rig>(
	"../../../../../art/animals/*/rig.json",
	{
		eager: true,
		import: "default",
	},
);
const svgFiles = import.meta.glob<string>(
	"../../../../../art/animals/*/*.svg",
	{ eager: true, query: "?url&no-inline", import: "default" },
);
const clipFiles = import.meta.glob<Clip>("../../../../../art/clips/*.json", {
	eager: true,
	import: "default",
});

/** The built-in pack's id. */
export const CLASSIC = "classic";

/** Knock's own rigs, by animal id. */
export const RIGS: ReadonlyMap<string, Rig> = new Map(
	Object.values(rigFiles).map((r) => [r.id, r]),
);

/** Knock's own clips: what every rig plays unless its pack replaces one. */
export const CLIPS = Object.fromEntries(
	Object.values(clipFiles).map((c) => [c.name, c]),
) as Record<ClipName, Clip>;

/** The fallback for an `animal` id with no art (lead decision, Phase 3). */
export const FALLBACK_ANIMAL = "fox";

/** `sheet.json` as the core checked it (ADR 0038). */
export interface SheetJson {
	image: string;
	frameWidth: number;
	frameHeight: number;
	pixelArt?: boolean;
	clips: Partial<Record<ClipName, SheetClip>>;
}

export interface SheetClip {
	row: number;
	frames: number;
	fps: number;
	loop: boolean;
}

/** A sprite-sheet character of the active pack. */
export interface SheetLook {
	kind: "sheet";
	pack: string;
	id: string;
	sheet: SheetJson;
	url: string;
	clips: Record<ClipName, Clip>;
}

export type Look = { kind: "rig"; rig: Rig } | SheetLook;

interface ActivePack {
	id: string;
	/** Its overflow den's URL, or null for Knock's own mound. */
	den: string | null;
	rigs: Map<string, Rig>;
	sheets: Map<string, SheetLook>;
	clips: Record<ClipName, Clip>;
}

let active: ActivePack | null = null;
/** Which pack each rig came from, for its image URLs and texture keys. */
const owner = new WeakMap<Rig, string>();
for (const rig of RIGS.values()) owner.set(rig, CLASSIC);
let version = 0;
const listeners = new Set<() => void>();

/**
 * Makes `data` the active pack (`null`: classic only). Its rigs win over Knock's own for
 * their ids; its clips replace the shared ones for every character. Sprite-sheet characters
 * aren't drawn yet (step 4) and fall back to Knock's own art.
 */
export function setActivePack(data: PackData | null): void {
	if (!data) active = null;
	else {
		const rigs = new Map<string, Rig>();
		const sheets = new Map<string, SheetLook>();
		for (const c of data.characters) {
			if (c.kind === "sheet" && c.sheet) {
				const sheet = c.sheet as SheetJson;
				sheets.set(c.id, {
					kind: "sheet",
					pack: data.id,
					id: c.id,
					sheet,
					url: packUrl(data.id, `characters/${c.id}/${sheet.image}`),
					clips: sheetClips(sheet),
				});
			}
			if (c.kind !== "rig" || !c.rig) continue;
			const rig = c.rig as Rig;
			owner.set(rig, data.id);
			rigs.set(c.id, rig);
		}
		active = {
			id: data.id,
			den: data.den ? packUrl(data.id, "den.svg") : null,
			rigs,
			sheets,
			clips: { ...CLIPS, ...(data.clips as Partial<Record<ClipName, Clip>>) },
		};
	}
	version += 1;
	for (const f of listeners) f();
}

/** For `useSyncExternalStore`: the strip draws again when the pack changes. */
export function subscribePack(f: () => void): () => void {
	listeners.add(f);
	return () => listeners.delete(f);
}

export function packVersion(): number {
	return version;
}

/** The active pack's overflow den, or null for Knock's own mound. */
export function denUrl(): string | null {
	return active?.den ?? null;
}

/** The clips every character plays now. */
export function currentClips(): Record<ClipName, Clip> {
	return active?.clips ?? CLIPS;
}

/** What to draw for a session's `animal`: the active pack's sheet or rig, else a rig. */
export function lookFor(animal: string): Look {
	const sheet = active?.sheets.get(animal);
	return sheet ?? { kind: "rig", rig: rigFor(animal) };
}

/** The clips `animal` plays: its sheet's own timings, or the rig clips. */
export function clipsFor(animal: string): Record<ClipName, Clip> {
	return active?.sheets.get(animal)?.clips ?? currentClips();
}

/** ADR 0038: a sheet's missing optional clips play these instead. */
const SHEET_FALLBACK: Partial<Record<ClipName, ClipName>> = {
	alert: "knock",
	pace: "wait",
	present: "rest",
};

/** The sheet row a clip uses, after fallbacks; `null` for the still pose. */
function sheetSource(sheet: SheetJson, clip: ClipName): SheetClip | null {
	const own = sheet.clips[clip];
	if (own) return own;
	const instead = SHEET_FALLBACK[clip];
	return (instead && sheet.clips[instead]) || null;
}

/**
 * A sheet's clips for the controller: each lasts its frames at its fps. Clips the sheet
 * doesn't draw take their fallback's timing; `step_up` and `step_down` without frames keep
 * Knock's own timing, while the renderer lifts the still pose.
 */
export function sheetClips(sheet: SheetJson): Record<ClipName, Clip> {
	const out = {} as Record<ClipName, Clip>;
	for (const name of Object.keys(CLIPS) as ClipName[]) {
		const src = sheetSource(sheet, name);
		out[name] = {
			name,
			loop: src ? src.loop : CLIPS[name].loop,
			durationMs: src
				? Math.max(1, (src.frames / src.fps) * 1000)
				: CLIPS[name].durationMs,
			tracks: {},
			swaps: [],
		};
	}
	return out;
}

/**
 * The cell to show `t` ms into `clip` (`null`: the still pose, the first frame of `wait`).
 * A looping row wraps; a play-once row holds its last frame.
 */
export function sheetCell(
	sheet: SheetJson,
	clip: ClipName | null,
	t: number,
): { row: number; col: number } {
	const src = (clip && sheetSource(sheet, clip)) || null;
	if (!src) return { row: sheet.clips.wait?.row ?? 0, col: 0 };
	const i = Math.floor((Math.max(0, t) / 1000) * src.fps);
	const col = src.loop ? i % src.frames : Math.min(i, src.frames - 1);
	return { row: src.row, col };
}

/** The rig to draw for a session's `animal`: the active pack's, else Knock's own, else the fox. */
export function rigFor(animal: string): Rig {
	const rig =
		active?.rigs.get(animal) ?? RIGS.get(animal) ?? RIGS.get(FALLBACK_ANIMAL);
	if (!rig) throw new Error("art: the fox is missing");
	return rig;
}

/** The pack a rig came from (classic for Knock's own). */
export function packOf(rig: Rig): string {
	return owner.get(rig) ?? CLASSIC;
}

/** `knockpack://` URL of a file in an installed pack, in Tauri's per-platform form. */
export function packUrl(pack: string, path: string): string {
	const convert = globalThis.__TAURI__?.core.convertFileSrc;
	const full = `${pack}/${path}`;
	return convert
		? convert(full, "knockpack")
		: `knockpack://localhost/${encodeURIComponent(full)}`;
}

/** URL of one part's SVG. */
export function partUrl(rig: Rig, file: string): string {
	const pack = packOf(rig);
	if (pack !== CLASSIC) return packUrl(pack, `characters/${rig.id}/${file}`);
	const url = svgFiles[`../../../../../art/animals/${rig.id}/${file}`];
	if (!url) throw new Error(`art: ${rig.id}/${file} is missing`);
	return url;
}

/** URL of one of Knock's own part SVGs, for previews. */
export function classicPartUrl(
	animal: string,
	file: string,
): string | undefined {
	return svgFiles[`../../../../../art/animals/${animal}/${file}`];
}
