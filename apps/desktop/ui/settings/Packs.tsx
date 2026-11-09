// Settings → Characters (ADR 0038): which sprite pack sessions get their characters from.
// Every pack shows its cast at rest; a pack that failed Knock's checks is listed, greyed,
// with the reasons, and can't be chosen. Images of installed packs come from `knockpack:`,
// served by the core from memory. A pack is added from one `.knockpack` file (Add a pack…,
// a drop on this window, or a double-click in the file manager): Knock checks it first and
// asks before installing, showing the cast it would add.
import {
	type CSSProperties,
	useCallback,
	useEffect,
	useId,
	useRef,
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
import { errorText, invoke, invokeBytes } from "./core";

/** `offer_pack_*`: a pack waiting for the user's yes (ADR 0038). */
interface Offer {
	/** For `install_pack`; null when the pack can't be installed. */
	token: string | null;
	pack: PackSummary;
	/** A pack with the same id is installed already. */
	replaces: boolean;
	data?: PackData;
}

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
	const [notice, setNotice] = useState<string | null>(null);
	const [offer, setOffer] = useState<Offer | null>(null);
	const [removing, setRemoving] = useState<string | null>(null);
	const [dragging, setDragging] = useState(false);
	const picker = useRef<HTMLInputElement>(null);

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

	const run = useCallback(async (work: Promise<unknown>) => {
		setError(null);
		setNotice(null);
		try {
			await work;
		} catch (e) {
			setError(errorText(e));
		}
	}, []);

	const offerPath = useCallback(
		(path: string) =>
			run(invoke<Offer>("offer_pack_path", { path }).then(setOffer)),
		[run],
	);

	useEffect(() => {
		load();
		void listen<null>("packs-changed", load);
		// A .knockpack Knock was opened with (a double-click), now or later.
		const take = () =>
			void invoke<Offer | null>("take_offered_pack").then(
				(o) => o && setOffer(o),
				() => {},
			);
		take();
		void listen<null>("pack-offered", take);
		// A file or folder dropped on this window.
		void listen<{ paths: string[] }>("tauri://drag-drop", (p) => {
			setDragging(false);
			const path = p.paths[0];
			if (path) void offerPath(path);
		});
		void listen<null>("tauri://drag-enter", () => setDragging(true));
		void listen<null>("tauri://drag-leave", () => setDragging(false));
	}, [load, offerPath]);

	const choose = (pack: string) =>
		run(
			onChoose(pack).then((e) => {
				if (e) throw new Error(e);
			}),
		);
	const reload = () => run(invoke<PackSummary[]>("reload_packs").then(show));
	const openFolder = () => run(invoke("open_packs_folder"));
	const pickFile = async (file: File | undefined) => {
		if (!file) return;
		const bytes = new Uint8Array(await file.arrayBuffer());
		await run(invokeBytes<Offer>("offer_pack_bytes", bytes).then(setOffer));
	};
	const install = (o: Offer, use: boolean) =>
		run(
			invoke<PackSummary[]>("install_pack", {
				token: o.token,
				replace: o.replaces,
			}).then(async (list) => {
				setOffer(null);
				await show(list);
				const added = list.find((p) => p.displayName === o.pack.displayName);
				if (use && added) await choose(added.id);
				setNotice(`Added ${o.pack.displayName}.`);
			}),
		);
	const remove = (pack: PackSummary) =>
		run(
			invoke<PackSummary[]>("remove_pack", { id: pack.id }).then(
				async (list) => {
					setRemoving(null);
					await show(list);
					setNotice(`Removed ${pack.displayName}.`);
				},
			),
		);

	const chosen = packs?.find((p) => p.id === value);
	const missing = packs !== null && !(chosen?.valid ?? false);

	return (
		<section
			aria-labelledby={`${id}t`}
			data-dragging={dragging ? "" : undefined}
		>
			<h2 id={`${id}t`}>Characters</h2>
			<p className="help">
				Which cast your sessions get. Switching changes the characters on screen
				at once. To add a pack, choose Add a pack, drop a .knockpack file on
				this window, or double-click it in your files.
			</p>
			{offer && (
				<OfferPanel
					offer={offer}
					onInstall={(use) => void install(offer, use)}
					onCancel={() => setOffer(null)}
				/>
			)}
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
							removing={removing === p.id}
							onRemove={(sure) =>
								sure
									? void remove(p)
									: setRemoving(removing === p.id ? null : p.id)
							}
						/>
					))}
				</div>
			)}
			{notice && <p role="status">{notice}</p>}
			{error && (
				<p className="error" role="alert">
					{error}
				</p>
			)}
			<div className="actions start">
				<button
					type="button"
					className="primary"
					onClick={() => picker.current?.click()}
				>
					Add a pack…
				</button>
				<input
					ref={picker}
					type="file"
					accept=".knockpack,.zip"
					hidden
					onChange={(e) => {
						void pickFile(e.currentTarget.files?.[0]);
						e.currentTarget.value = "";
					}}
				/>
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

/** "Add this pack?": the cast it would add, or why it can't be added. */
function OfferPanel({
	offer: o,
	onInstall,
	onCancel,
}: {
	offer: Offer;
	onInstall(use: boolean): void;
	onCancel(): void;
}) {
	const p = o.pack;
	const byline = [p.author && `by ${p.author}`, p.license]
		.filter(Boolean)
		.join(" · ");
	// Settings is long: the question comes into view however it arrived.
	const box = useRef<HTMLFieldSetElement>(null);
	// biome-ignore lint/correctness/useExhaustiveDependencies: once per new offer
	useEffect(() => {
		box.current?.scrollIntoView({ block: "center" });
	}, [o]);
	return (
		<fieldset className="offer" ref={box}>
			<legend className="visually-hidden">Add {p.displayName}</legend>
			{o.token ? (
				<>
					<p>
						<strong>Add “{p.displayName}”?</strong> {p.characters.length}{" "}
						character
						{p.characters.length === 1 ? "" : "s"}
						{byline && ` · ${byline}`}
					</p>
					{p.description && <p className="help">{p.description}</p>}
					<span className="pack-cast" aria-hidden="true">
						{p.characters.slice(0, PICTURED).map((c) => (
							<Preview
								key={c.id}
								pack={p}
								data={o.data ?? null}
								character={c}
							/>
						))}
						{p.characters.length > PICTURED && (
							<span className="preview more">
								+{p.characters.length - PICTURED}
							</span>
						)}
					</span>
					{o.replaces && (
						<p role="status">
							A pack with the same name is installed. Adding this one replaces
							it.
						</p>
					)}
					<div className="actions start">
						<button
							type="button"
							className="primary"
							onClick={() => onInstall(true)}
						>
							{o.replaces ? "Replace and use it" : "Add and use it"}
						</button>
						<button type="button" onClick={() => onInstall(false)}>
							{o.replaces ? "Replace" : "Just add it"}
						</button>
						<button type="button" onClick={onCancel}>
							Cancel
						</button>
					</div>
				</>
			) : (
				<>
					<p role="alert">
						<strong>{p.displayName} can't be added.</strong>
					</p>
					<ul className="pack-errors">
						{p.errors.map((e) => (
							<li key={e}>{e}</li>
						))}
					</ul>
					<div className="actions start">
						<button type="button" onClick={onCancel}>
							Close
						</button>
					</div>
				</>
			)}
		</fieldset>
	);
}

function PackRow({
	pack: p,
	data,
	checked,
	name,
	onChoose,
	removing,
	onRemove,
}: {
	pack: PackSummary;
	data: PackData | null;
	checked: boolean;
	name: string;
	onChoose(): void;
	/** The "Remove …? Yes / No" question is showing. */
	removing: boolean;
	/** `true`: confirmed; `false`: ask, or stop asking. */
	onRemove(sure: boolean): void;
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
			{!p.builtin && (
				<div className="pack-remove">
					{removing ? (
						<>
							<span>Remove {p.displayName} from Knock?</span>
							<button type="button" onClick={() => onRemove(true)}>
								Remove
							</button>
							<button type="button" onClick={() => onRemove(false)}>
								Keep it
							</button>
						</>
					) : (
						<button
							type="button"
							className="link"
							onClick={() => onRemove(false)}
						>
							Remove…
						</button>
					)}
				</div>
			)}
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
