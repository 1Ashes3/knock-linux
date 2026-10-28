// `pnpm art:check` (§8.7, §12.1). Run by Node directly (type stripping), so imports keep `.ts`.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
	MAX_COLORS,
	REQUIRED_CLIPS,
	REQUIRED_PARTS,
	TRACK_FIELDS,
	VIEW_BOX,
} from "./format.ts";

/** Art files keyed by path relative to the art root, with `/` separators. */
export type ArtFiles = Map<string, string>;

export function readArt(root: string): ArtFiles {
	const files: ArtFiles = new Map();
	for (const d of readdirSync(root, { recursive: true, withFileTypes: true })) {
		if (!d.isFile()) continue;
		const path = join(d.parentPath, d.name);
		files.set(
			relative(root, path).replaceAll("\\", "/"),
			readFileSync(path, "utf8"),
		);
	}
	return files;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj =>
	typeof v === "object" && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number =>
	typeof v === "number" && Number.isFinite(v);

function parseJson(text: string, where: string, errors: string[]): unknown {
	try {
		return JSON.parse(text);
	} catch {
		errors.push(`${where}: not valid JSON`);
		return undefined;
	}
}

const SVG_ROOT = /^\s*(<\?xml[^>]*\?>\s*)?<svg[\s>]/;
const NOT_FLAT =
	/<(script|foreignObject|image|linearGradient|radialGradient|filter|pattern)\b|\son\w+\s*=/i;
const COLOR = /\b(?:fill|stroke|stop-color)\s*[=:]\s*["']?([^"';\s>)]+\)?)/gi;

function colorsIn(svg: string): string[] {
	const out: string[] = [];
	for (const m of svg.matchAll(COLOR)) {
		let c = (m[1] ?? "").toLowerCase();
		if (c === "none" || c === "transparent") continue;
		if (/^#[0-9a-f]{3}$/.test(c))
			c = `#${[...c.slice(1)].map((h) => h + h).join("")}`;
		out.push(c);
	}
	return out;
}

/** Returns every problem found; an empty list means the art is valid. */
export function checkArt(files: ArtFiles): string[] {
	const errors: string[] = [];
	const animalParts = new Map<string, Set<string>>();

	const rigPaths = [...files.keys()].filter((p) =>
		/^animals\/[^/]+\/rig\.json$/.test(p),
	);
	if (rigPaths.length === 0) errors.push("no animals found in art/animals");

	for (const rigPath of rigPaths) {
		const dir = rigPath.split("/")[1] as string;
		const rig = parseJson(files.get(rigPath) as string, rigPath, errors);
		if (!isObj(rig)) {
			if (rig !== undefined) errors.push(`${rigPath}: must be an object`);
			continue;
		}
		if (rig.id !== dir) errors.push(`${rigPath}: id must be "${dir}"`);
		if (typeof rig.displayName !== "string" || rig.displayName === "")
			errors.push(`${rigPath}: displayName is missing`);
		if (JSON.stringify(rig.viewBox) !== JSON.stringify(VIEW_BOX))
			errors.push(`${rigPath}: viewBox must be ${JSON.stringify(VIEW_BOX)}`);
		const parts = Array.isArray(rig.parts) ? rig.parts : [];
		if (!Array.isArray(rig.parts))
			errors.push(`${rigPath}: parts must be a list`);

		const names = new Set<string>();
		const colors = new Set<string>();
		for (const p of parts) {
			if (!isObj(p) || typeof p.name !== "string") {
				errors.push(`${rigPath}: every part needs a name`);
				continue;
			}
			const where = `${dir}/${p.name}`;
			if (names.has(p.name)) errors.push(`${where}: duplicate part`);
			names.add(p.name);
			if (
				!Array.isArray(p.pivot) ||
				p.pivot.length !== 2 ||
				!p.pivot.every(isNum)
			)
				errors.push(`${where}: pivot must be [x, y]`);
			if (!isNum(p.z)) errors.push(`${where}: z must be a number`);
			if (typeof p.file !== "string" || !p.file.endsWith(".svg")) {
				errors.push(`${where}: file must be an .svg file`);
				continue;
			}
			const svg = files.get(`animals/${dir}/${p.file}`);
			if (svg === undefined) {
				errors.push(`${where}: file ${p.file} is missing`);
				continue;
			}
			if (!SVG_ROOT.test(svg) || !svg.trimEnd().endsWith("</svg>")) {
				errors.push(`${where}: ${p.file} is not an SVG file`);
				continue;
			}
			if (!svg.includes(`viewBox="${VIEW_BOX.join(" ")}"`))
				errors.push(
					`${where}: ${p.file} must use viewBox="${VIEW_BOX.join(" ")}"`,
				);
			if (NOT_FLAT.test(svg))
				errors.push(
					`${where}: ${p.file} must be flat shapes only (no scripts, images, gradients, filters or event handlers)`,
				);
			for (const c of colorsIn(svg)) colors.add(c);
		}
		for (const r of REQUIRED_PARTS)
			if (!names.has(r)) errors.push(`${dir}: missing required part ${r}`);
		if (colors.size > MAX_COLORS)
			errors.push(
				`${dir}: uses ${colors.size} colors, at most ${MAX_COLORS} allowed (${[...colors].join(", ")})`,
			);
		animalParts.set(dir, names);
	}

	// Clips are shared, so every part a clip names must exist in every animal.
	const missingIn = (part: string) =>
		[...animalParts].filter(([, n]) => !n.has(part)).map(([id]) => id);

	const clipNames = new Set<string>();
	for (const [path, text] of files) {
		const m = /^clips\/([^/]+)\.json$/.exec(path);
		if (!m) continue;
		const file = m[1] as string;
		clipNames.add(file);
		const clip = parseJson(text, path, errors);
		if (!isObj(clip)) {
			if (clip !== undefined) errors.push(`${path}: must be an object`);
			continue;
		}
		for (const k of Object.keys(clip))
			if (!["name", "loop", "durationMs", "tracks", "swaps"].includes(k))
				errors.push(`${path}: unknown field ${k}`);
		if (clip.name !== file) errors.push(`${path}: name must be "${file}"`);
		if (typeof clip.loop !== "boolean")
			errors.push(`${path}: loop must be true or false`);
		const d = clip.durationMs;
		if (!isNum(d) || d <= 0) {
			errors.push(`${path}: durationMs must be a positive number`);
			continue;
		}
		const inClip = (v: unknown) => isNum(v) && v >= 0 && v <= d;

		if (!isObj(clip.tracks)) errors.push(`${path}: tracks must be an object`);
		for (const [part, keys] of Object.entries(
			isObj(clip.tracks) ? clip.tracks : {},
		)) {
			const where = `${path}: track ${part}`;
			const lacking = missingIn(part);
			if (lacking.length)
				errors.push(`${where}: unknown part (not in ${lacking.join(", ")})`);
			if (!Array.isArray(keys) || keys.length === 0) {
				errors.push(`${where}: must be a non-empty list of keyframes`);
				continue;
			}
			let prev = -1;
			for (const k of keys) {
				if (!isObj(k) || !inClip(k.t)) {
					errors.push(`${where}: every keyframe needs t between 0 and ${d}`);
					continue;
				}
				if ((k.t as number) <= prev)
					errors.push(`${where}: keyframes must be in increasing t order`);
				prev = k.t as number;
				for (const [f, v] of Object.entries(k)) {
					if (f === "t") continue;
					if (!(TRACK_FIELDS as readonly string[]).includes(f))
						errors.push(`${where}: unknown track field ${f}`);
					else if (!isNum(v)) errors.push(`${where}: ${f} must be a number`);
				}
			}
		}

		if (!Array.isArray(clip.swaps)) {
			errors.push(`${path}: swaps must be a list`);
			continue;
		}
		for (const s of clip.swaps) {
			if (!isObj(s)) {
				errors.push(`${path}: every swap must be an object`);
				continue;
			}
			for (const k of Object.keys(s))
				if (!["from", "to", "start", "end"].includes(k))
					errors.push(`${path}: unknown swap field ${k}`);
			for (const part of [s.from, s.to]) {
				if (typeof part !== "string") {
					errors.push(`${path}: swap needs from and to part names`);
					continue;
				}
				const lacking = missingIn(part);
				if (lacking.length)
					errors.push(
						`${path}: swap names unknown part ${part} (not in ${lacking.join(", ")})`,
					);
			}
			if (
				!inClip(s.start) ||
				!inClip(s.end) ||
				(s.start as number) >= (s.end as number)
			)
				errors.push(
					`${path}: swap needs 0 <= start < end <= durationMs (${d})`,
				);
		}
	}
	for (const r of REQUIRED_CLIPS)
		if (!clipNames.has(r)) errors.push(`missing required clip ${r}`);

	return errors;
}

export const ART_ROOT = resolve(import.meta.dirname, "../../../../art");

if (
	process.argv[1] &&
	resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
	const errors = checkArt(readArt(ART_ROOT));
	for (const e of errors) console.error(`art:check: ${e}`);
	if (errors.length) process.exitCode = 1;
	else console.log("art:check: all animals and clips are valid");
}
