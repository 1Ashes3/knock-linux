// Keyboard focus for a confirm panel (§10.5): the panel takes focus when it opens, and
// focus goes back to what opened it when it closes.
import { type RefObject, useEffect } from "react";

/**
 * Focuses `panel` (give it `tabIndex={-1}`) on mount. On unmount, focuses whatever had
 * focus when the panel opened, else `fallback` (the button that opened it), if it is
 * still on the page.
 */
export function usePanelFocus(
	panel: RefObject<HTMLElement | null>,
	fallback?: RefObject<HTMLElement | null>,
): void {
	// biome-ignore lint/correctness/useExhaustiveDependencies: once per opening
	useEffect(() => {
		const active = document.activeElement;
		const before =
			active instanceof HTMLElement &&
			active !== document.body &&
			!panel.current?.contains(active)
				? active
				: null;
		panel.current?.focus({ preventScroll: true });
		return () => {
			const back = before?.isConnected ? before : fallback?.current;
			if (back?.isConnected) back.focus();
		};
	}, []);
}
