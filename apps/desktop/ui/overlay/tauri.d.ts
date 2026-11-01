// Injected by Tauri when the core sets `withGlobalTauri: true`. Absent in a plain browser.
declare var __TAURI__:
	| {
			core: {
				invoke: (
					cmd: string,
					args?: Record<string, unknown>,
				) => Promise<unknown>;
			};
			event: {
				listen: <T>(
					event: string,
					handler: (e: { payload: T }) => void,
				) => Promise<() => void>;
			};
	  }
	| undefined;
