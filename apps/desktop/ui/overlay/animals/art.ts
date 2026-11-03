// Every animal and clip in art/ (SPEC.md §8.7), bundled at build time. New animals are
// picked up without code changes. Part SVGs are separate files (`no-inline`), because the
// app's CSP allows images only from the app itself, not data: or blob: URLs.
import type { Clip, Rig } from "../../gallery/format.ts";
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

export const RIGS: ReadonlyMap<string, Rig> = new Map(
	Object.values(rigFiles).map((r) => [r.id, r]),
);

export const CLIPS = Object.fromEntries(
	Object.values(clipFiles).map((c) => [c.name, c]),
) as Record<ClipName, Clip>;

/** The fallback for an `animal` id with no art (lead decision, Phase 3). */
export const FALLBACK_ANIMAL = "fox";

/** The rig to draw for a session's `animal`; unknown ids get the fox. */
export function rigFor(animal: string): Rig {
	const rig = RIGS.get(animal) ?? RIGS.get(FALLBACK_ANIMAL);
	if (!rig) throw new Error("art: the fox is missing");
	return rig;
}

/** URL of one part's SVG. */
export function partUrl(rig: Rig, file: string): string {
	const url = svgFiles[`../../../../../art/animals/${rig.id}/${file}`];
	if (!url) throw new Error(`art: ${rig.id}/${file} is missing`);
	return url;
}
