# Knock art

The format is SPEC.md §8.7. This file records only the details §8.7 leaves open.
The TypeScript types live in `apps/desktop/ui/gallery/format.ts`.

- `pnpm gallery` shows every animal in every clip, on a light or dark taskbar.
- `pnpm art:check` validates everything here.

## Parts

- Every part SVG uses `viewBox="0 0 64 64"`, the same as the rig, and is drawn in place. Stacking all parts in `z` order gives the rig pose.
- `pivot` is in viewBox units. Rotation and scale happen around it.
- Parts are independent: there is no parent/child link. A clip that moves the head also moves the ears and eyes.
- In the rig pose `eyes_closed` is hidden, and every other part is shown.
- The `card` is drawn behind the body, where the body covers it. `present` moves it out.
- The sleep "z" is shared UI, not art: it is drawn by `apps/desktop/ui/gallery/zzz.ts`, never by a rig or clip.
- Part SVGs carry `aria-hidden="true"`: they are decoration. The UI labels the whole character.
- Flat shapes only. No scripts, images, gradients, filters or event handlers. At most 6 colors per animal, counted over `fill`, `stroke` and `stop-color` across all its parts.

## Clips

```json
{
	"name": "rest",
	"loop": true,
	"durationMs": 4000,
	"tracks": { "card": [{ "t": 0, "x": -15, "y": -12, "rot": -8 }] },
	"swaps": [{ "from": "eyes_open", "to": "eyes_closed", "start": 3000, "end": 3500 }]
}
```

- `name` matches the file name. `tracks` and `swaps` are always present, even when empty.
- `t` is in milliseconds from the start of the clip, from 0 to `durationMs`. Keyframes are in increasing `t` order.
- Track values are relative to the rig pose:
  - `x` and `y` are offsets in viewBox units; up is negative `y`.
  - `rot` is in degrees; clockwise is positive.
  - `scaleX`, `scaleY` and `alpha` are multipliers.
  - A field not in the track stays at the rig pose (0 for offsets, 1 for multipliers).
- Each field is eased on its own, using only the keyframes that set it. The easing is always sine ease-in-out between keyframes. Before the first keyframe the value holds, and after the last one it holds too.
- A swap hides `from` and shows `to` while `start <= t < end` (in ms).
- `loop: true` repeats from `t = 0`. A looping clip should end with the same values it starts with.
- `loop: false` plays once and holds its last frame. What plays next (`present` then `rest`, `step_up` then `knock`) is for the character controller (Phase 3) to decide. A clip file never names another clip. To keep chained clips smooth:
  - `rest` starts in the pose where `present` ends, holding the card.
  - `step_up` ends with every part 12 units up, and `step_down` starts there. `knock` is drawn in place, so the controller keeps the 12-unit offset while it plays.
- The gallery replays play-once clips after a one-second pause.
