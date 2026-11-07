// The "While you were away" card (SPEC.md FR-9, §10.3, §10.5, ADR 0034). It sits to the
// left of the strip, bottom edges level, so it never covers a character or its chip. The
// whole card is one hit region and one button: a click folds it away.
import type { CSSProperties } from "react";
import {
	type AwayContent,
	type AwayMetrics,
	awayContent,
	awayHeight,
	awayMetrics,
	awayName,
	moreText,
	OUTCOME_GLYPH,
} from "./away";
import type { Size } from "./bridge";
import { CloseIcon, GlyphIcon } from "./Glyph";
import { TIER_FILL } from "./pill";
import type { ShownAway } from "./store";

export interface CardLayout {
	content: AwayContent;
	m: AwayMetrics;
	width: number;
	height: number;
	name: string;
}

/** What the card shows and how big it is; null while no card shows. */
export function cardLayout(
	shown: ShownAway | null,
	size: Size,
): CardLayout | null {
	if (!shown) return null;
	const content = awayContent(shown.summary, shown.at);
	const m = awayMetrics(size);
	return {
		content,
		m,
		width: m.width,
		height: awayHeight(content, m),
		name: awayName(content),
	};
}

/**
 * The live region that makes Narrator announce the card when it appears. Always
 * mounted, so the change is announced (an announcer added with its text is often missed).
 */
export function AwayAnnouncer({ card }: { card: CardLayout | null }) {
	return (
		<div className="sr-only" role="status" aria-live="polite">
			{card?.name ?? ""}
		</div>
	);
}

export function AwayCard({
	card,
	style,
	onDismiss,
}: {
	card: CardLayout;
	/** Where it goes; the strip decides. */
	style: CSSProperties;
	onDismiss: () => void;
}) {
	const { content, m } = card;
	return (
		<div
			className="away"
			role="button"
			aria-label={card.name}
			data-hit=""
			onClick={onDismiss}
			style={
				{
					...style,
					width: card.width,
					height: card.height,
					fontSize: m.font,
					lineHeight: `${m.lineHeight}px`,
					"--line": `${m.lineHeight}px`,
					"--row": `${m.row}px`,
				} as CSSProperties
			}
		>
			<div className="away-head" aria-hidden="true">
				<span>{content.heading}</span>
				<CloseIcon />
			</div>
			{content.rows.length > 0 && (
				<ul aria-hidden="true">
					{content.rows.map((r) => (
						<li key={r.id} data-outcome={r.outcome}>
							<GlyphIcon g={OUTCOME_GLYPH[r.outcome]} />
							<span className="away-project">{r.project}</span>
							<span
								className="away-outcome"
								style={
									{
										"--fill":
											TIER_FILL[r.outcome === "ready" ? "ready" : "needs_you"],
									} as CSSProperties
								}
							>
								{r.label}
							</span>
						</li>
					))}
				</ul>
			)}
			{content.more > 0 && (
				<div className="away-more" aria-hidden="true">
					{moreText(content.more)}
				</div>
			)}
		</div>
	);
}
