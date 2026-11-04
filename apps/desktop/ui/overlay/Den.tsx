// The den (SPEC.md FR-10, §10.3, §8.7): the 6th slot, holding every session beyond 5.
// It shows one count per non-zero tier, each with the tier's glyph and color, so color is
// never the only signal (§6), and Narrator reads it in words (§10.5). Drawn by the UI in
// both modes: a pill in plain mode, a burrow mound in animals mode. Not clickable (lead
// decision, v0.3), so it is never a hit region.
import type { CSSProperties } from "react";
import type { SessionView } from "./bridge";
import { GlyphIcon } from "./Glyph";
import { denCounts, denName, TIER_FILL, tierGlyph } from "./pill";

/** The mound's three flat colors and its dark outline, in the 64x64 slot viewBox. */
export const DEN_ART = {
	outline: "#2a1a12",
	earth: "#c99a5b",
	hole: "#5b3a22",
} as const;

export function Den({
	hidden,
	look,
}: {
	hidden: SessionView[];
	look: "pill" | "mound";
}) {
	const counts = denCounts(hidden);
	// One column of larger counts for 1 or 2 tiers; two columns for 3 or 4.
	const cols = counts.length > 2 ? 2 : 1;
	return (
		<div
			className={`den den-${look}`}
			role="img"
			aria-label={denName(hidden)}
			data-cols={cols}
			style={{ "--cols": cols } as CSSProperties}
		>
			{look === "mound" && (
				<svg className="den-art" viewBox="0 0 64 64" aria-hidden="true">
					<path
						d="M2 62.5C2 31 14.5 9 32 9C49.5 9 62 31 62 62.5Z"
						fill={DEN_ART.earth}
						stroke={DEN_ART.outline}
						strokeWidth="2.5"
						strokeLinejoin="round"
					/>
					<path
						d="M23.5 62.5C23.5 53 27 48.5 32 48.5C37 48.5 40.5 53 40.5 62.5Z"
						fill={DEN_ART.hole}
						stroke={DEN_ART.outline}
						strokeWidth="2"
						strokeLinejoin="round"
					/>
				</svg>
			)}
			<div className="den-counts" aria-hidden="true">
				{counts.map(({ tier, count }) => (
					<span
						key={tier}
						className="den-count"
						data-tier={tier}
						style={{ "--fill": TIER_FILL[tier] } as CSSProperties}
					>
						<GlyphIcon g={tierGlyph(tier)} />
						<b>{count}</b>
					</span>
				))}
			</div>
		</div>
	);
}
