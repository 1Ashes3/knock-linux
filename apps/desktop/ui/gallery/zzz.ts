// The shared sleep "z" (SPEC.md §8.7): drawn by the UI over any animal, never part of a rig.
// Coordinates are in the 64x64 rig viewBox, so it scales with the character.

const SVG_NS = "http://www.w3.org/2000/svg";
const STYLE_ID = "knock-zzz-style";

// Light fill with the same dark outline as the animals reads on light and dark taskbars.
// One letter starts every 1.6 s, once per breath of the sleep clip.
// Reduced motion (Windows "Animation effects" off) leaves one still "Z".
const CSS = `
.knock-zzz text {
	font-family: "Segoe UI", system-ui, sans-serif;
	font-weight: 700;
	fill: #ffffff;
	stroke: #2a1a12;
	stroke-width: 1.5px;
	stroke-linejoin: round;
	paint-order: stroke;
	opacity: 0;
	animation: knock-zzz 4.8s ease-out infinite backwards;
}
.knock-zzz text:nth-child(2) { animation-delay: 1.6s; }
.knock-zzz text:nth-child(3) { animation-delay: 3.2s; }
@keyframes knock-zzz {
	0% { opacity: 0; transform: translate(0, 0); }
	20% { opacity: 1; }
	100% { opacity: 0; transform: translate(4px, -6px); }
}
@media (prefers-reduced-motion: reduce) {
	.knock-zzz text { animation: none; }
	.knock-zzz text:nth-child(2) { opacity: 1; }
}
`;

/** "z Z Z" along a rising diagonal, starting just right of the head: [text, x, y, font size]. */
const LETTERS: readonly [string, number, number, number][] = [
	["z", 47, 14, 8],
	["Z", 52, 7, 10],
	["Z", 58, -1, 12],
];

/** Returns a decorative `<g>` to append to an animal's `<svg>` while it sleeps. */
export function createZzz(doc: Document): SVGGElement {
	if (!doc.getElementById(STYLE_ID)) {
		const style = doc.createElement("style");
		style.id = STYLE_ID;
		style.textContent = CSS;
		doc.head.append(style);
	}
	const g = doc.createElementNS(SVG_NS, "g");
	g.setAttribute("class", "knock-zzz");
	g.setAttribute("aria-hidden", "true");
	for (const [letter, x, y, size] of LETTERS) {
		const t = doc.createElementNS(SVG_NS, "text");
		t.setAttribute("x", String(x));
		t.setAttribute("y", String(y));
		t.setAttribute("font-size", String(size));
		t.textContent = letter;
		g.append(t);
	}
	return g;
}
