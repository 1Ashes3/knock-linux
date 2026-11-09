// Settings → Characters (ADR 0038): which sprite pack sessions get their characters from.
// Every pack shows its cast at rest; a pack that failed Knock's checks is listed, greyed,
// with the reasons, and can't be chosen. Images of installed packs come from `knockpack:`,
// served by the core from memory.
import {
	type CSSProperties,
	useCallback,
	useEffect,
	useId,
	useState,
} from "react";
import type { Rig } from "../gallery/format.ts";
import { CLASSIC, classicPartUrl, packUrl, RIGS } from "../overlay/animals/art";
import {
	listen,
	type PackCharacterInfo,
	type PackData,
	type PackSummary,
} from "../overlay/bridge";
import { errorText, invoke } from "./core";

/** Preview size in CSS px. */
const SIZE = 48;
/** Characters pictured per pack; a "+N" badge counts the rest. */
const PICTURED = 6;

interface SheetJson {
	image: string;
	frameWidth: number;
	frameHeight: number;
	pixelArt?: boolean;
	clips: Record<string, { row: number }>;
}

export function Characters({
	value,
	onChoose,
}: {
	value: string;
	onChoose(id: string): Promise<string | null>;
}) {
	const id = useId();
	const [packs, setPacks] = useState<PackSummary[] | null>(null);
	const [data, setData] = useState<Record<string, PackData | null>>({});
	const [error, setError] = useState<string | null>(null);

	const show = useCallback(async (list: PackSummary[]) => {
		setPacks(list);
		const entries = await Promise.all(
			list
				.filter((p) => p.valid && !p.builtin)
				.map(
					async (p) =>
						[
							p.id,
							await invoke<PackData | null>("get_pack", { id: p.id }),
						] as const,
				),
		);
		setData(Object.fromEntries(entries));
	}, []);

	const load = useCallback(
		() =>
			void invoke<PackSummary[]>("list_packs").then(show, (e) =>
				setError(errorText(e)),
			),
		[show],
	);

	useEffect(() => {
		load();
		void listen<null>("packs-changed", load);
	}, [load]);

	const run = async (work: Promise<unknown>) => {
		setError(null);
		try {
			await work;
		} catch (e) {
			setError(errorText(e));
		}
	};
	const choose = (pack: string) =>
		run(
			onChoose(pack).then((e) => {
				if (e) throw new Error(e);
			}),
		);
	const reload = () => run(invoke<PackSummary[]>("reload_packs").then(show));
	const openFolder = () => run(invoke("open_packs_folder"));

	const chosen = packs?.find((p) => p.id === value);
	const missing = packs !== null && !(chosen?.valid ?? false);

	return (
		<section aria-labelledby={`${id}t`}>
			<h2 id={`${id}t`}>Characters</h2>
			<p className="help">
				Which cast your sessions get. Switching changes the characters on screen
				at once.
			</p>
			{missing && (
				<p role="status">
					The pack you chose isn't installed or can't be used, so Knock is
					showing Classic.
				</p>
			)}
			{packs === null ? (
				<p className="help">Looking for packs…</p>
			) : (
				<div className="packs" role="radiogroup" aria-labelledby={`${id}t`}>
					{packs.map((p) => (
						<PackRow
							key={p.id}
							pack={p}
							data={data[p.id] ?? null}
							checked={p.id === value || (missing && p.id === CLASSIC)}
							name={`${id}r`}
							onChoose={() => void choose(p.id)}
						/>
					))}
				</div>
			)}
			{error && (
				<p className="error" role="alert">
					{error}
				</p>
			)}
			<div className="actions start">
				<button type="button" onClick={() => void reload()}>
					Reload packs
				</button>
				<button type="button" onClick={() => void openFolder()}>
					Open packs folder
				</button>
			</div>
		</section>
	);
}

function PackRow({
	pack: p,
	data,
	checked,
	name,
	onChoose,
}: {
	pack: PackSummary;
	data: PackData | null;
	checked: boolean;
	name: string;
	onChoose(): void;
}) {
	const byline = [p.author && `by ${p.author}`, p.license]
		.filter(Boolean)
		.join(" · ");
	return (
		<div className="pack" data-invalid={p.valid ? undefined : ""}>
			<label className="pack-choice">
				<input
					type="radio"
					name={name}
					checked={checked}
					disabled={!p.valid}
					onChange={onChoose}
				/>
				<span className="pack-text">
					<strong>{p.displayName}</strong>
					{byline && <span className="help">{byline}</span>}
					{p.description && <span className="help">{p.description}</span>}
					<span className="visually-hidden">
						{p.characters.length} character
						{p.characters.length === 1 ? "" : "s"}
					</span>
				</span>
				<span className="pack-cast" aria-hidden="true">
					{p.characters.slice(0, PICTURED).map((c) => (
						<Preview key={c.id} pack={p} data={data} character={c} />
					))}
					{p.characters.length > PICTURED && (
						<span className="preview more">
							+{p.characters.length - PICTURED}
						</span>
					)}
				</span>
			</label>
			{!p.valid && (
				<details className="pack-errors">
					<summary>Why it can't be used</summary>
					<ul>
						{p.errors.map((e) => (
							<li key={e}>{e}</li>
						))}
					</ul>
				</details>
			)}
		</div>
	);
}

/** One character at rest, `SIZE` px square. */
function Preview({
	pack,
	data,
	character: c,
}: {
	pack: PackSummary;
	data: PackData | null;
	character: PackCharacterInfo;
}) {
	if (pack.builtin) {
		const rig = RIGS.get(c.id);
		return rig ? (
			<RigPreview rig={rig} url={(f) => classicPartUrl(c.id, f) ?? ""} />
		) : null;
	}
	const full = data?.characters.find((x) => x.id === c.id);
	if (full?.kind === "rig" && full.rig)
		return (
			<RigPreview
				rig={full.rig as Rig}
				url={(f) => packUrl(pack.id, `characters/${c.id}/${f}`)}
			/>
		);
	if (full?.kind === "sheet" && full.sheet)
		return (
			<SheetPreview
				sheet={full.sheet as SheetJson}
				url={packUrl(
					pack.id,
					`characters/${c.id}/${(full.sheet as SheetJson).image}`,
				)}
			/>
		);
	return <span className="preview" title={c.displayName} />;
}

/** Every part stacked in `z` order: the rig pose (`eyes_closed` is hidden at rest). */
function RigPreview({ rig, url }: { rig: Rig; url(file: string): string }) {
	const parts = [...rig.parts]
		.filter((p) => p.name !== "eyes_closed")
		.sort((a, b) => a.z - b.z);
	return (
		<span className="preview" title={rig.displayName}>
			{parts.map((p) => (
				<img key={p.name} src={url(p.file)} alt="" draggable={false} />
			))}
		</span>
	);
}

/** The first frame of `wait`: a sheet character's rest pose. */
function SheetPreview({ sheet, url }: { sheet: SheetJson; url: string }) {
	const row = sheet.clips.wait?.row ?? 0;
	const scale = SIZE / Math.max(sheet.frameWidth, sheet.frameHeight);
	const frame: CSSProperties = {
		width: sheet.frameWidth,
		height: sheet.frameHeight,
		transform: `scale(${scale})`,
	};
	const image: CSSProperties = {
		top: -row * sheet.frameHeight,
		imageRendering: sheet.pixelArt ? "pixelated" : "auto",
	};
	return (
		<span className="preview">
			<span className="sheet-frame" style={frame}>
				<img src={url} alt="" draggable={false} style={image} />
			</span>
		</span>
	);
}
