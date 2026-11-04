// The shared sleep "z" (SPEC.md §8.7): drawn by the UI over any animal, never part of a rig.
// Coordinates are in the 64x64 rig viewBox, so it scales with the character.
// Styles live in zzz.css (a file, not an injected <style>, so the overlay's CSP allows it).
import "./zzz.css";

const SVG_NS = "http://www.w3.org/2000/svg";

/** "z Z Z" along a rising diagonal, starting just right of the head: [text, x, y, font size]. */
export const ZZZ_LETTERS: readonly [string, number, number, number][] = [
	["z", 47, 14, 8],
	["Z", 52, 7, 10],
	["Z", 58, -1, 12],
];

/** Returns a decorative `<g>` to append to an animal's `<svg>` while it sleeps. */
export function createZzz(doc: Document): SVGGElement {
	const g = doc.createElementNS(SVG_NS, "g");
	g.setAttribute("class", "knock-zzz");
	g.setAttribute("aria-hidden", "true");
	for (const [letter, x, y, size] of ZZZ_LETTERS) {
		const t = doc.createElementNS(SVG_NS, "text");
		t.setAttribute("x", String(x));
		t.setAttribute("y", String(y));
		t.setAttribute("font-size", String(size));
		t.textContent = letter;
		g.append(t);
	}
	return g;
}
