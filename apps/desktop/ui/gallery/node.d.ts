// The few Node APIs art:check uses, so the UI needs no @types/node.
// ponytail: hand-written subset; replace with @types/node if more Node APIs are needed.
declare module "node:fs" {
	interface Dirent {
		name: string;
		parentPath: string;
		isFile(): boolean;
	}
	export function readdirSync(
		path: string,
		options: { recursive: true; withFileTypes: true },
	): Dirent[];
	export function readFileSync(path: string, encoding: "utf8"): string;
}
declare module "node:path" {
	export function join(...parts: string[]): string;
	export function relative(from: string, to: string): string;
	export function resolve(...parts: string[]): string;
}
declare module "node:url" {
	export function fileURLToPath(url: string): string;
}
declare var process: { argv: string[]; exitCode?: number };
interface ImportMeta {
	dirname: string;
}
