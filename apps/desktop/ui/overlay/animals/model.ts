// Which characters are on screen, in which slot, playing what (SPEC.md §10.1, §10.3).
// Pure: the previous result and the time are passed in, so every step can be tested.
import type { SessionView } from "../bridge";
import { layout, MAX_VISIBLE } from "../pill";
import {
	type Clips,
	changesAt,
	endedFadeStart,
	FADE_MS,
	nextPlan,
	type Plan,
	settlesAt,
} from "./controller";

export interface Character {
	session: SessionView;
	/** 0 is nearest the clock. */
	slot: number;
	plan: Plan;
	/** When it starts fading out (ended after 60 s, or gone from the list); null if never. */
	fadeFrom: number | null;
	/** Gone from the core's list: drawn while it fades, but no chip, button or name. */
	leaving: boolean;
}

export interface Model {
	characters: Character[];
	/** Sessions in the den (FR-10). */
	hidden: SessionView[];
	/** Slots in use, including the den. */
	slots: number;
	/** The next time something changes without a new event; Infinity if nothing will. */
	wakeAt: number;
}

interface Ghost {
	character: Character;
	leftAt: number;
}

/** What one step remembers for the next. */
export interface Memory {
	plans: ReadonlyMap<string, Plan>;
	ghosts: ReadonlyMap<string, Ghost>;
	/** Characters on screen last time, in slot order. */
	shown: readonly Character[];
}

export const EMPTY_MEMORY: Memory = {
	plans: new Map(),
	ghosts: new Map(),
	shown: [],
};

/** Gone for good: an ended session after its fade (§10.1). */
function faded(s: SessionView, now: number, still: boolean): boolean {
	const from = endedFadeStart(s);
	return from !== null && now >= from + (still ? 0 : FADE_MS);
}

export function step(
	sessions: SessionView[],
	memory: Memory,
	still: boolean,
	now: number,
	/** Each character's clips: a sprite sheet times them by its own frames (ADR 0038). */
	clipsFor: (animal: string) => Clips,
	/** The `idleMotion` setting: working sprites flourish now and then. */
	idle = false,
	/** Characters with a slot; the rest go into the den (the demo shows them all). */
	cap = MAX_VISIBLE,
): { model: Model; memory: Memory } {
	const live = sessions.filter((s) => !faded(s, now, still));
	const { visible, hidden } = layout(live, cap);
	const ids = new Set(sessions.map((s) => s.id));

	// A session that leaves the list fades out where it was (motion only).
	const ghosts = new Map<string, Ghost>();
	for (const [id, g] of memory.ghosts)
		if (!ids.has(id) && now < g.leftAt + FADE_MS) ghosts.set(id, g);
	if (!still)
		memory.shown.forEach((c) => {
			const id = c.session.id;
			if (c.leaving || ids.has(id) || ghosts.has(id)) return;
			ghosts.set(id, {
				leftAt: now,
				character: { ...c, fadeFrom: now, leaving: true },
			});
		});

	// Den sessions keep a plan too, so one that comes out of the den (or goes back in)
	// without a state change of its own appears settled: only a state change moves an
	// animal (§10.1). One that steps out because it entered Needs you has a new plan,
	// so it steps up and knocks.
	const plans = new Map<string, Plan>();
	for (const session of live)
		plans.set(
			session.id,
			nextPlan(
				memory.plans.get(session.id),
				session,
				still,
				now,
				clipsFor(session.animal),
				idle,
			),
		);
	const order: Character[] = visible.map((session) => ({
		session,
		slot: 0,
		plan: plans.get(session.id) as Plan,
		fadeFrom: endedFadeStart(session),
		leaving: false,
	}));
	// Ghosts keep their old slot while they fade, if there is room for them.
	const byOldSlot = [...ghosts.values()].sort(
		(a, b) => a.character.slot - b.character.slot,
	);
	for (const g of byOldSlot) {
		if (order.length >= cap) {
			ghosts.delete(g.character.session.id);
			continue;
		}
		order.splice(Math.min(g.character.slot, order.length), 0, g.character);
		plans.set(g.character.session.id, g.character.plan);
	}
	const characters = order.map((c, slot) => ({ ...c, slot }));
	for (const g of ghosts.values()) {
		const placed = characters.find(
			(c) => c.session.id === g.character.session.id,
		);
		if (placed) g.character = placed;
	}

	let wakeAt = Number.POSITIVE_INFINITY;
	const later = (t: number) => {
		if (t > now && t < wakeAt) wakeAt = t;
	};
	for (const c of characters) {
		const clips = clipsFor(c.session.animal);
		later(settlesAt(c.plan, clips));
		// Idle motion pauses between flourishes: wake for each one.
		later(changesAt(c.plan, now, clips));
		if (c.fadeFrom !== null) {
			later(c.fadeFrom);
			later(c.fadeFrom + FADE_MS);
		}
	}

	return {
		model: {
			characters,
			hidden,
			slots: characters.length + (hidden.length ? 1 : 0),
			wakeAt,
		},
		memory: { plans, ghosts, shown: characters },
	};
}
