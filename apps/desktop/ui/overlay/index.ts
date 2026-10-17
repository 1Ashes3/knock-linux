import { sendHitRegions } from "./hitRegions";
import {
	call,
	firstSession,
	getSessions,
	type SessionView,
	sessionLabel,
} from "./sessions";

const target = document.getElementById("target") as HTMLElement;
const label = document.getElementById("label") as HTMLElement;
let clicks = 0;
let sessions: SessionView[] = [];

target.addEventListener("click", () => {
	clicks++;
	target.textContent = String(clicks);
	target.setAttribute("aria-label", `Test target, clicks: ${clicks}`);
	const first = firstSession(sessions);
	if (first) void call("jump_to_session", { id: first.id });
});

function report(): void {
	const { x, y, width, height } = target.getBoundingClientRect();
	sendHitRegions([{ x, y, width, height }]);
}

// ponytail: spike-only 2 s poll; replace with the `sessions-changed` event (§8.5).
async function poll(): Promise<void> {
	sessions = await getSessions();
	label.textContent = sessionLabel(firstSession(sessions));
}

report();
window.addEventListener("resize", report);
void poll();
setInterval(poll, 2000);
