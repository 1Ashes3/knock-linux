import { sendHitRegions } from "./hitRegions";

const target = document.getElementById("target") as HTMLElement;
let clicks = 0;

target.addEventListener("click", () => {
	clicks++;
	target.textContent = String(clicks);
	target.setAttribute("aria-label", `Test target, clicks: ${clicks}`);
});

function report(): void {
	const { x, y, width, height } = target.getBoundingClientRect();
	sendHitRegions([{ x, y, width, height }]);
}

report();
window.addEventListener("resize", report);
