import {
	type Clip,
	HIDDEN_AT_REST,
	type Keyframe,
	type TrackField,
} from "./format.ts";

export type Pose = Record<TrackField, number> & { visible: boolean };

const REST: Record<TrackField, number> = {
	x: 0,
	y: 0,
	rot: 0,
	scaleX: 1,
	scaleY: 1,
	alpha: 1,
};

/** Looping clips wrap; once clips stop and hold on their last frame. */
export function clipTime(clip: Clip, elapsedMs: number): number {
	const d = clip.durationMs;
	if (clip.loop) return ((elapsedMs % d) + d) % d;
	return Math.min(Math.max(elapsedMs, 0), d);
}

const ease = (u: number) => (1 - Math.cos(Math.PI * u)) / 2;

/** Sine ease-in-out between the keyframes that set `field`; holds before the first and after the last. */
export function sample(keys: Keyframe[], field: TrackField, t: number): number {
	const ks = keys.filter((k) => k[field] !== undefined);
	const first = ks[0];
	if (!first) return REST[field];
	let a = first;
	for (const b of ks) {
		if (b.t > t) {
			if (b === a) break;
			const u = (t - a.t) / (b.t - a.t);
			const va = a[field] as number;
			return va + ((b[field] as number) - va) * ease(u);
		}
		a = b;
	}
	return a[field] as number;
}

export function poseAt(
	clip: Clip,
	parts: string[],
	t: number,
): Record<string, Pose> {
	const out: Record<string, Pose> = {};
	for (const name of parts) {
		const keys = clip.tracks[name] ?? [];
		out[name] = {
			x: sample(keys, "x", t),
			y: sample(keys, "y", t),
			rot: sample(keys, "rot", t),
			scaleX: sample(keys, "scaleX", t),
			scaleY: sample(keys, "scaleY", t),
			alpha: sample(keys, "alpha", t),
			visible: !HIDDEN_AT_REST.includes(name),
		};
	}
	for (const s of clip.swaps) {
		if (s.start <= t && t < s.end) {
			const from = out[s.from];
			const to = out[s.to];
			if (from) from.visible = false;
			if (to) to.visible = true;
		}
	}
	return out;
}

/** SVG transform: offset by (x, y), then rotate and scale around the pivot. */
export function partTransform(pivot: [number, number], p: Pose): string {
	const [px, py] = pivot;
	return `translate(${p.x + px} ${p.y + py}) rotate(${p.rot}) scale(${p.scaleX} ${p.scaleY}) translate(${-px} ${-py})`;
}
