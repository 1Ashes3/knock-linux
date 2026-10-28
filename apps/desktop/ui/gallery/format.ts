// The §8.7 art format. One module, used by the gallery player and by art:check.
// Details §8.7 leaves open are decided in art/README.md.

export const REQUIRED_PARTS = [
	"body",
	"head",
	"ear_l",
	"ear_r",
	"eyes_open",
	"eyes_closed",
	"paw_l",
	"paw_r",
	"tail",
	"card",
] as const;

export const REQUIRED_CLIPS = [
	"sleep",
	"work",
	"wait",
	"step_up",
	"step_down",
	"knock",
	"alert",
	"pace",
	"present",
	"rest",
] as const;

export const TRACK_FIELDS = [
	"x",
	"y",
	"rot",
	"scaleX",
	"scaleY",
	"alpha",
] as const;
export type TrackField = (typeof TRACK_FIELDS)[number];

/** Parts not drawn in the rig pose; they appear only while a swap shows them. */
export const HIDDEN_AT_REST: readonly string[] = ["eyes_closed"];

export const VIEW_BOX = [0, 0, 64, 64] as const;
export const MAX_COLORS = 6;

export interface RigPart {
	name: string;
	file: string;
	/** Rotation and scale origin, in viewBox units. */
	pivot: [number, number];
	/** Draw order; higher is in front. */
	z: number;
}

export interface Rig {
	id: string;
	displayName: string;
	viewBox: [number, number, number, number];
	parts: RigPart[];
}

/** `t` is ms from clip start. Other fields are optional, relative to the rig pose. */
export type Keyframe = { t: number } & Partial<Record<TrackField, number>>;

/** While `start <= t < end` (ms), `from` is hidden and `to` is shown. */
export interface Swap {
	from: string;
	to: string;
	start: number;
	end: number;
}

export interface Clip {
	name: string;
	loop: boolean;
	durationMs: number;
	/** Part name → keyframes, sorted by `t`. */
	tracks: Record<string, Keyframe[]>;
	swaps: Swap[];
}
