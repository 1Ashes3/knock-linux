// Injected by Tauri when the core sets `withGlobalTauri: true`. Absent in a plain browser.
declare var __TAURI__:
	| {
			core: {
				invoke: (
					cmd: string,
					/** Named arguments, or a raw body (a file's bytes). */
					args?: Record<string, unknown> | Uint8Array,
				) => Promise<unknown>;
				/** A URL for `path` under a custom protocol, in the platform's form. */
				convertFileSrc?: (path: string, protocol?: string) => string;
			};
			event: {
				listen: <T>(
					event: string,
					handler: (e: { payload: T }) => void,
				) => Promise<() => void>;
			};
	  }
	| undefined;
