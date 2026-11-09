// The PixiJS renderer for animals mode (SPEC.md §7.1, §8.7, §6). Draws each rig from its
// part SVGs and samples clips with the gallery's player (sine ease-in-out, holds, swaps).
//
// - Pixi's Canvas 2D renderer: a handful of small sprites needs no WebGL context, and its
//   output matches the browser's own SVG drawing.
// - Parts are drawn once per size and screen density, at exactly the device pixels they
//   cover, so animals stay crisp at 100%, 150% and 200% scaling.
// - The loop runs at most 30 fps, and only while something moves. Once every character
//   has settled it stops completely (§6 CPU, ADR 0031) until the next `sync`.
import "pixi.js/unsafe-eval"; // the app's CSP has no 'unsafe-eval'
import {
	autoDetectRenderer,
	CanvasSource,
	Container,
	Rectangle,
	type Renderer,
	Sprite,
	Texture,
	Ticker,
} from "pixi.js";
import type { Clip, Rig } from "../../gallery/format.ts";
import { poseAt } from "../../gallery/player.ts";
import {
	clipsFor,
	lookFor,
	packOf,
	partUrl,
	type SheetLook,
	sheetCell,
} from "./art";
import {
	type Animated,
	anyAnimating,
	fadeAlpha,
	frameAt,
	LIFT_UNITS,
	type Plan,
} from "./controller";

const FRAME_MS = 1000 / 30;
const UNITS = 64; // rig viewBox size (§8.7)
/** No tracks and no swaps: every part at the rig pose. */
const RIG_POSE: Clip = {
	name: "rig",
	loop: false,
	durationMs: 1,
	tracks: {},
	swaps: [],
};

/** One character to draw, in logical px within the canvas. */
export interface DrawCharacter extends Animated {
	id: string;
	animal: string;
	x: number;
	y: number;
}

export interface View {
	width: number;
	height: number;
	/** Slot size in logical px. */
	s: number;
	dpr: number;
}

type Parts = Map<string, Texture>;

interface RigView {
	kind: "rig";
	root: Container;
	body: Container;
	sprites: Map<string, Sprite>;
	rig: Rig;
	texKey: string;
}

/** A sprite-sheet character (ADR 0038): one sprite showing a cell of its sheet. */
interface SheetView {
	kind: "sheet";
	root: Container;
	body: Container;
	sprite: Sprite;
	look: SheetLook;
	texKey: string;
}

type CharacterView = RigView | SheetView;

/** Loads a sprite sheet once, at its own size; cells are cut from it as needed. */
async function loadSheet(url: string, pixelArt: boolean): Promise<Texture> {
	const img = new Image();
	img.crossOrigin = "anonymous";
	img.src = url;
	await img.decode();
	const canvas = document.createElement("canvas");
	canvas.width = img.naturalWidth;
	canvas.height = img.naturalHeight;
	canvas.getContext("2d")?.drawImage(img, 0, 0);
	return new Texture({
		source: new CanvasSource({
			resource: canvas,
			scaleMode: pixelArt ? "nearest" : "linear",
		}),
	});
}

/** Sine ease-in-out from 0 to 1, as the clip player eases. */
const ease = (x: number) =>
	(1 - Math.cos(Math.PI * Math.min(1, Math.max(0, x)))) / 2;

/** Rasterises one part SVG to a canvas of `px` device pixels. */
async function rasterise(url: string, px: number): Promise<Texture> {
	const img = new Image();
	// Pack images come from `knockpack:`, which allows this; the canvas stays usable.
	img.crossOrigin = "anonymous";
	img.src = url;
	await img.decode();
	const canvas = document.createElement("canvas");
	canvas.width = px;
	canvas.height = px;
	canvas.getContext("2d")?.drawImage(img, 0, 0, px, px);
	return new Texture({
		source: new CanvasSource({ resource: canvas, resolution: px / UNITS }),
	});
}

export class AnimalRenderer {
	private renderer: Renderer | null = null;
	private readonly stage = new Container();
	private readonly chars = new Map<string, CharacterView>();
	private readonly textures = new Map<string, Promise<Parts>>();
	private readonly loaded = new Map<string, Parts>();
	/** Sprite sheets by pack and character, and the cells cut from them. */
	private readonly sheets = new Map<string, Promise<Texture>>();
	private readonly sheetsLoaded = new Map<string, Texture>();
	private readonly cells = new Map<string, Texture>();
	private list: DrawCharacter[] = [];
	private view: View | null = null;
	private raf = 0;
	private lastFrame = 0;
	private running = false;
	private destroyed = false;
	private readonly host: HTMLElement;

	/**
	 * @param host where the canvas goes. Its `data-loop` says whether the loop is
	 *   "running" or "stopped", and `data-drawn` how many characters are drawn.
	 */
	constructor(host: HTMLElement) {
		this.host = host;
		host.dataset.loop = "stopped";
		host.dataset.drawn = "0";
	}

	async init(view: View): Promise<void> {
		// Pixi drives its texture garbage collector from a system ticker whose frame loop
		// never stops. Textures here are freed by hand, so it stays off (§6 CPU).
		Ticker.system.autoStart = false;
		Ticker.system.stop();
		const r = await autoDetectRenderer({
			preference: "canvas",
			width: Math.max(1, view.width),
			height: Math.max(1, view.height),
			resolution: view.dpr,
			autoDensity: true,
			backgroundAlpha: 0,
			roundPixels: true,
			manageImports: false, // no Pixi accessibility layer or pointer events
		});
		if (this.destroyed) {
			r.destroy();
			return;
		}
		r.canvas.setAttribute("aria-hidden", "true");
		this.host.append(r.canvas);
		this.renderer = r;
		this.view = null;
		this.sync(this.list, view);
	}

	/** Shows `list` now, then animates while anything moves. */
	sync(list: DrawCharacter[], view: View): void {
		this.list = list;
		const r = this.renderer;
		if (!r) {
			this.view = view;
			return;
		}
		const old = this.view;
		if (
			!old ||
			old.width !== view.width ||
			old.height !== view.height ||
			old.dpr !== view.dpr
		)
			r.resize(Math.max(1, view.width), Math.max(1, view.height), view.dpr);
		this.view = view;

		const keep = new Set(list.map((c) => c.id));
		for (const [id, cv] of this.chars)
			if (!keep.has(id)) {
				cv.root.destroy({ children: true });
				this.chars.delete(id);
			}
		for (const c of list) this.ensure(c, view);
		// Parts drawn for another size or density are no longer used.
		const px = `@${Math.round(view.s * view.dpr)}`;
		for (const [key, parts] of this.loaded)
			if (!key.endsWith(px)) {
				for (const t of parts.values()) t.destroy(true);
				this.loaded.delete(key);
				this.textures.delete(key);
			}
		this.frame();
	}

	private texKey(rig: Rig, view: View): string {
		return `${packOf(rig)}:${rig.id}@${Math.round(view.s * view.dpr)}`;
	}

	private parts(rig: Rig, view: View): Parts | undefined {
		const key = this.texKey(rig, view);
		const ready = this.loaded.get(key);
		if (ready) return ready;
		if (!this.textures.has(key)) {
			const px = Math.round(view.s * view.dpr);
			const load = Promise.all(
				rig.parts.map(
					async (p) =>
						[p.name, await rasterise(partUrl(rig, p.file), px)] as const,
				),
			).then((entries) => new Map(entries));
			this.textures.set(key, load);
			load.then(
				(parts) => {
					if (this.destroyed) {
						for (const t of parts.values()) t.destroy(true);
						return;
					}
					this.loaded.set(key, parts);
					if (this.view) this.sync(this.list, this.view);
				},
				(e) => console.warn(`Could not load the ${rig.id}`, e),
			);
		}
		return undefined;
	}

	/** Creates or updates the sprites for one character. */
	private ensure(c: DrawCharacter, view: View): void {
		const look = lookFor(c.animal);
		if (look.kind === "sheet") {
			this.ensureSheet(c, look, view);
			return;
		}
		const rig = look.rig;
		const key = this.texKey(rig, view);
		let cv = this.chars.get(c.id);
		if (cv && cv.texKey !== key) {
			cv.root.destroy({ children: true });
			this.chars.delete(c.id);
			cv = undefined;
		}
		if (!cv) {
			const parts = this.parts(rig, view);
			if (!parts) return; // drawn once its parts have loaded
			const root = new Container();
			const body = new Container();
			root.addChild(body);
			const sprites = new Map<string, Sprite>();
			for (const part of [...rig.parts].sort((a, b) => a.z - b.z)) {
				const sprite = new Sprite(parts.get(part.name));
				sprite.anchor.set(part.pivot[0] / UNITS, part.pivot[1] / UNITS);
				body.addChild(sprite);
				sprites.set(part.name, sprite);
			}
			this.stage.addChild(root);
			cv = { kind: "rig", root, body, sprites, rig, texKey: key };
			this.chars.set(c.id, cv);
		}
		cv.root.position.set(c.x, c.y);
		cv.root.scale.set(view.s / UNITS);
	}

	private sheet(look: SheetLook): Texture | undefined {
		const key = `${look.pack}:${look.id}`;
		const ready = this.sheetsLoaded.get(key);
		if (ready) return ready;
		if (!this.sheets.has(key)) {
			const load = loadSheet(look.url, look.sheet.pixelArt ?? false);
			this.sheets.set(key, load);
			load.then(
				(t) => {
					if (this.destroyed) {
						t.destroy(true);
						return;
					}
					this.sheetsLoaded.set(key, t);
					if (this.view) this.sync(this.list, this.view);
				},
				(e) => console.warn(`Could not load the ${look.id} sheet`, e),
			);
		}
		return undefined;
	}

	/** The texture of one cell of a loaded sheet. */
	private cell(
		look: SheetLook,
		base: Texture,
		row: number,
		col: number,
	): Texture {
		const key = `${look.pack}:${look.id}:${row}:${col}`;
		let t = this.cells.get(key);
		if (!t) {
			const { frameWidth: w, frameHeight: h } = look.sheet;
			t = new Texture({
				source: base.source,
				frame: new Rectangle(col * w, row * h, w, h),
			});
			this.cells.set(key, t);
		}
		return t;
	}

	private ensureSheet(c: DrawCharacter, look: SheetLook, view: View): void {
		const key = `${look.pack}:${look.id}:sheet`;
		let cv = this.chars.get(c.id);
		if (cv && cv.texKey !== key) {
			cv.root.destroy({ children: true });
			this.chars.delete(c.id);
			cv = undefined;
		}
		if (!cv) {
			const base = this.sheet(look);
			if (!base) return; // drawn once its sheet has loaded
			const root = new Container();
			const body = new Container();
			root.addChild(body);
			const { frameWidth: w, frameHeight: h } = look.sheet;
			const sprite = new Sprite(this.cell(look, base, 0, 0));
			// Into the rig's 64-unit square: centred, standing on its bottom edge.
			const scale = UNITS / Math.max(w, h);
			sprite.scale.set(scale);
			sprite.position.set((UNITS - w * scale) / 2, UNITS - h * scale);
			body.addChild(sprite);
			this.stage.addChild(root);
			cv = { kind: "sheet", root, body, sprite, look, texKey: key };
			this.chars.set(c.id, cv);
		}
		cv.root.position.set(c.x, c.y);
		cv.root.scale.set(view.s / UNITS);
	}

	/** Draws one frame, then keeps the loop going only while something moves. */
	private frame = (): void => {
		const r = this.renderer;
		if (!r || this.destroyed) return;
		const now = Date.now();
		for (const c of this.list) {
			const cv = this.chars.get(c.id);
			if (cv) this.pose(cv, c.animal, c.plan, c.fadeFrom, now);
		}
		r.render(this.stage);
		this.lastFrame = now;
		this.host.dataset.drawn = String(this.chars.size);
		const pending = this.list.some((c) => !this.chars.has(c.id));
		const moving = this.list.some((c) =>
			anyAnimating([c], now, clipsFor(c.animal)),
		);
		if (!pending && moving) this.schedule();
		else this.stop();
	};

	private pose(
		cv: CharacterView,
		animal: string,
		plan: Plan,
		fadeFrom: number | null,
		now: number,
	): void {
		cv.root.alpha =
			fadeFrom === null ? 1 : fadeAlpha(fadeFrom, now, plan.still);
		if (cv.kind === "sheet") {
			this.poseSheet(cv, plan, now);
			return;
		}
		const clips = clipsFor(animal);
		const f = frameAt(plan, now, clips);
		const names = [...cv.sprites.keys()];
		const poses = poseAt(f.clip ? clips[f.clip] : RIG_POSE, names, f.t);
		for (const part of cv.rig.parts) {
			const sprite = cv.sprites.get(part.name);
			const p = poses[part.name];
			if (!sprite || !p) continue;
			sprite.position.set(part.pivot[0] + p.x, part.pivot[1] + p.y);
			sprite.rotation = (p.rot * Math.PI) / 180;
			sprite.scale.set(p.scaleX, p.scaleY);
			sprite.alpha = p.alpha;
			sprite.visible = p.visible;
		}
		cv.body.position.set(0, -f.lift);
	}

	/** A sheet shows one cell; stepping up and down lifts the whole sprite. */
	private poseSheet(cv: SheetView, plan: Plan, now: number): void {
		const { look } = cv;
		const f = frameAt(plan, now, look.clips);
		const base = this.sheetsLoaded.get(`${look.pack}:${look.id}`);
		if (base) {
			const { row, col } = sheetCell(look.sheet, f.clip, f.t);
			cv.sprite.texture = this.cell(look, base, row, col);
		}
		let lift = f.lift;
		if (f.clip === "step_up" || f.clip === "step_down") {
			const p = ease(f.t / look.clips[f.clip].durationMs);
			lift = LIFT_UNITS * (f.clip === "step_up" ? p : 1 - p);
		}
		cv.body.position.set(0, -lift);
	}

	private schedule(): void {
		this.setRunning(true);
		if (this.raf) return;
		const tick = (): void => {
			// 30 fps at most: skip display frames until a 30 fps frame is due.
			if (Date.now() - this.lastFrame < FRAME_MS - 4) {
				this.raf = requestAnimationFrame(tick);
				return;
			}
			this.raf = 0;
			this.frame();
		};
		this.raf = requestAnimationFrame(tick);
	}

	private stop(): void {
		if (this.raf) cancelAnimationFrame(this.raf);
		this.raf = 0;
		this.setRunning(false);
	}

	private setRunning(running: boolean): void {
		if (this.running === running) return;
		this.running = running;
		this.host.dataset.loop = running ? "running" : "stopped";
	}

	destroy(): void {
		this.destroyed = true;
		if (this.raf) cancelAnimationFrame(this.raf);
		this.raf = 0;
		this.stage.destroy({ children: true });
		for (const parts of this.loaded.values())
			for (const t of parts.values()) t.destroy(true);
		for (const t of this.cells.values()) t.destroy(false);
		for (const t of this.sheetsLoaded.values()) t.destroy(true);
		this.renderer?.destroy({ removeView: true });
		this.renderer = null;
	}
}
