/// <reference types="vite/client" />
import { type Clip, REQUIRED_CLIPS, type Rig } from "./format.ts";
import { clipTime, partTransform, poseAt } from "./player.ts";
import { createZzz } from "./zzz.ts";

// What each clip is for (§8.7 table), shown under each cell.
const USED_FOR: Record<string, string> = {
	sleep: "Idle",
	work: "Working",
	wait: "Waiting on another agent",
	step_up: "Stepping out of the strip",
	step_down: "Stepping back in",
	knock: "Needs input",
	alert: "Failed",
	pace: "Stalled",
	present: "Ready",
	rest: "Ready, after presenting",
};
/** Pause before a play-once clip starts again. */
const REPLAY_PAUSE_MS = 1000;
const SVG_NS = "http://www.w3.org/2000/svg";

const rigs = Object.values(
	import.meta.glob<Rig>("../../../../art/animals/*/rig.json", {
		eager: true,
		import: "default",
	}),
);
const svgs = import.meta.glob<string>("../../../../art/animals/*/*.svg", {
	eager: true,
	query: "?raw",
	import: "default",
});
const allClips = Object.values(
	import.meta.glob<Clip>("../../../../art/clips/*.json", {
		eager: true,
		import: "default",
	}),
);
// Required clips first, in §8.7 order, then any extras.
const order = (c: Clip) => {
	const i = (REQUIRED_CLIPS as readonly string[]).indexOf(c.name);
	return i < 0 ? REQUIRED_CLIPS.length : i;
};
const clips = allClips.sort((a, b) => order(a) - order(b));

interface Cell {
	clip: Clip;
	rig: Rig;
	svg: SVGSVGElement;
	groups: Map<string, SVGGElement>;
}
const cells: Cell[] = [];
const main = document.getElementById("animals") as HTMLElement;

for (const rig of rigs) {
	const h2 = document.createElement("h2");
	h2.textContent = rig.displayName;
	const grid = document.createElement("div");
	grid.className = "grid";
	main.append(h2, grid);

	for (const clip of clips) {
		const svg = document.createElementNS(SVG_NS, "svg");
		svg.setAttribute("viewBox", rig.viewBox.join(" "));
		svg.setAttribute("role", "img");
		svg.setAttribute("aria-label", `${rig.displayName}, ${clip.name}`);
		const groups = new Map<string, SVGGElement>();
		for (const part of [...rig.parts].sort((a, b) => a.z - b.z)) {
			const g = document.createElementNS(SVG_NS, "g");
			// Our own art files, bundled at build time; never user or agent text.
			g.innerHTML =
				svgs[`../../../../art/animals/${rig.id}/${part.file}`] ?? "";
			svg.append(g);
			groups.set(part.name, g);
		}
		// The shared "z" (§8.7) goes over every sleeping animal, on top of its parts.
		if (clip.name === "sleep") svg.append(createZzz(document));
		const caption = document.createElement("figcaption");
		const title = document.createElement("strong");
		title.textContent = clip.name;
		caption.append(
			title,
			`${USED_FOR[clip.name] ?? "Extra clip"}. ${clip.loop ? "Loops" : "Plays once"}.`,
		);
		const figure = document.createElement("figure");
		figure.append(svg, caption);
		grid.append(figure);
		cells.push({ clip, rig, svg, groups });
	}
}

function frame(now: number): void {
	for (const { clip, rig, groups } of cells) {
		const elapsed = clip.loop ? now : now % (clip.durationMs + REPLAY_PAUSE_MS);
		const poses = poseAt(clip, [...groups.keys()], clipTime(clip, elapsed));
		for (const part of rig.parts) {
			const g = groups.get(part.name);
			const p = poses[part.name];
			if (!g || !p) continue;
			g.setAttribute("transform", partTransform(part.pivot, p));
			g.setAttribute("opacity", String(p.alpha));
			g.style.display = p.visible ? "" : "none";
		}
	}
	requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

const size = document.getElementById("size") as HTMLSelectElement;
function applySize(): void {
	const px = Number(size.value);
	for (const { svg } of cells) {
		svg.setAttribute("width", String(px));
		svg.setAttribute("height", String(px));
		// Room above for step_up and the raised card, which leave the 64x64 box.
		svg.style.marginTop = `${px / 4}px`;
	}
}
size.addEventListener("change", applySize);
applySize();

for (const radio of document.querySelectorAll<HTMLInputElement>(
	'input[name="theme"]',
)) {
	radio.addEventListener("change", () => {
		document.body.className = radio.value;
	});
}
