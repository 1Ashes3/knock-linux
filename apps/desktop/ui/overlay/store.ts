import { createStore } from "zustand/vanilla";
import type {
	AwaySummary,
	SessionsChanged,
	SessionView,
	Settings,
} from "./bridge";

/** An `away-summary` the card is showing, and when it arrived (the user's return). */
export interface ShownAway {
	summary: AwaySummary;
	at: number;
}

// Holds exactly what the core sends (§7.3). No state is derived here.
export interface StripState {
	/** Last applied `sessions-changed` rev; null until the first one. */
	rev: number | null;
	sessions: SessionView[];
	settings: Settings | null;
	/** The "While you were away" card, while it shows (FR-9). */
	away: ShownAway | null;
	/** `sessions-changed`: drops a rev that is not newer than the last one. */
	applySessions(payload: SessionsChanged): void;
	/**
	 * `get_sessions()` result. `{ rev, sessions }` goes through the rev check.
	 * A bare array has no rev, so it only fills an empty store: any event wins over it.
	 */
	applyInitialSessions(result: unknown): void;
	/** `settings-changed`: always the newest. */
	applySettings(settings: Settings): void;
	/** `get_settings()` result: only if no `settings-changed` arrived first. */
	applyInitialSettings(settings: unknown): void;
	/** `away-summary`: replaces any card still showing (ADR 0034). */
	applyAway(summary: AwaySummary, at?: number): void;
	/** The card folded away. */
	clearAway(): void;
}

export const createStripStore = () =>
	createStore<StripState>()((set, get) => ({
		rev: null,
		sessions: [],
		settings: null,
		away: null,
		applySessions(payload) {
			const { rev } = get();
			if (rev !== null && payload.rev <= rev) return;
			set({ rev: payload.rev, sessions: payload.sessions });
		},
		applyInitialSessions(result) {
			if (Array.isArray(result)) {
				if (get().rev === null) set({ sessions: result as SessionView[] });
			} else if (result && typeof result === "object" && "rev" in result) {
				get().applySessions(result as SessionsChanged);
			}
		},
		applySettings(settings) {
			set({ settings });
		},
		applyInitialSettings(settings) {
			if (settings && typeof settings === "object" && get().settings === null)
				set({ settings: settings as Settings });
		},
		applyAway(summary, at = Date.now()) {
			set({ away: { summary, at } });
		},
		clearAway() {
			set({ away: null });
		},
	}));

export type StripStore = ReturnType<typeof createStripStore>;
