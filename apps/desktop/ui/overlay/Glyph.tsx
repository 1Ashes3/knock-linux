// The tier glyphs (SPEC.md §6: color is never the only signal). Shared by the pills, the
// animals' badges and marker, the den and the away card.
import type { ReactNode } from "react";
import { type Glyph, INK, RED } from "./pill";

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

/** The white-on-dark "x" of the dismiss buttons and the away card. */
export function CloseIcon() {
	return (
		<svg viewBox="0 0 12 12" aria-hidden="true">
			<circle cx="6" cy="6" r="5.4" fill={INK} stroke="#fff" />
			<path
				d="M4 4L8 8M8 4L4 8"
				stroke="#fff"
				strokeWidth="1.5"
				strokeLinecap="round"
			/>
		</svg>
	);
}
