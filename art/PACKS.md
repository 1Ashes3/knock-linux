# Making a Knock sprite pack

A short guide for people who want to make a pack: a set of characters that Knock shows for your Claude Code sessions. The code in `crates/knock-core/src/packs.rs` is the final word. If this guide and the code disagree, the code wins.

## What a pack is

A pack is a cast of 1 to 16 characters, sent as one `.knockpack` file (a zip). Sessions get the characters in the order the pack lists them. Each character is either a vector rig (SVG parts that Knock moves) or a sprite sheet (a PNG of frames). A pack may also have its own clips and an optional den. Four characters per pack is a good size, because up to 5 show at once.

## Folder layout

```
my-pack/
  pack.json            required
  README.md            optional
  LICENSE              optional (LICENSE.txt or LICENSE.md also work)
  den.svg              optional
  clips/<clip>.json    optional, rig characters only
  characters/<id>/rig.json      a vector rig, with its part SVGs, or
  characters/<id>/sheet.json    a sprite sheet, with its one PNG
```

- Nothing else is allowed. No other folders, no other file types, no hidden files, no links.
- Each character folder may hold only the files that character uses. A spare SVG or a draft PNG fails the check.
- No folders inside a character folder.
- Keep previews and notes outside the pack folder.
- Name the pack folder the same as the pack's `id`. Knock rejects an installed folder with another name.

## pack.json

```json
{
  "format": 1,
  "id": "my-pack",
  "displayName": "My Pack",
  "author": "Your name or studio",
  "license": "CC0-1.0",
  "description": "One sentence about the cast.",
  "characters": ["first", "second", "third", "fourth"]
}
```

- `format`, `id`, `displayName` and `characters` are required. `author`, `license` and `description` are optional.
- Unknown fields are rejected. The id `classic` is reserved.
- `characters` lists the character folder names, in order. Every listed character needs its folder, and every folder must be listed.

## Characters: vector rig or sprite sheet

A character uses one kind or the other, not both. A pack can mix the two.

### Vector rig (`rig.json` and SVG parts)

`characters/<id>/rig.json`, with exactly these 10 parts, each once: `body head ear_l ear_r eyes_open eyes_closed paw_l paw_r tail card`.

```json
{
  "id": "sprout",
  "displayName": "Sprout",
  "viewBox": [0, 0, 64, 64],
  "parts": [
    { "name": "tail",        "file": "tail.svg",        "pivot": [40, 52], "z": 0 },
    { "name": "card",        "file": "card.svg",        "pivot": [32, 46], "z": 1 },
    { "name": "body",        "file": "body.svg",        "pivot": [32, 58], "z": 2 },
    { "name": "ear_l",       "file": "ear_l.svg",       "pivot": [21, 20], "z": 3 },
    { "name": "ear_r",       "file": "ear_r.svg",       "pivot": [43, 20], "z": 3 },
    { "name": "head",        "file": "head.svg",        "pivot": [32, 34], "z": 4 },
    { "name": "eyes_open",   "file": "eyes_open.svg",   "pivot": [32, 26], "z": 5 },
    { "name": "eyes_closed", "file": "eyes_closed.svg", "pivot": [32, 26], "z": 5 },
    { "name": "paw_l",       "file": "paw_l.svg",       "pivot": [23, 48], "z": 6 },
    { "name": "paw_r",       "file": "paw_r.svg",       "pivot": [41, 48], "z": 6 }
  ]
}
```

Each part is an SVG with `viewBox="0 0 64 64"`, drawn in place. Stacking the parts by `z` gives the rest pose. Example of a whole part file:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" aria-hidden="true">
<circle cx="32" cy="40" r="14" fill="#7cc6a4" stroke="#1d3b2f" stroke-width="1.5"/>
</svg>
```

- `eyes_closed` is hidden at rest. `card` is drawn behind the body.
- Layout guidance, not checked by the code: keep the character inside x 8 to 56, and keep the top of the head at y 12 or more. Knocking lifts the character 12 units, and anything above the canvas is cut off.
- Only the allowed SVG elements and attributes are kept. See "Limits and rejections" below. Export plain SVG and clean it up; editor exports often carry rejected metadata or ids.
- The part names are fixed, but what they show is up to you (wings, hair buns, a wand).

### Sprite sheet (`sheet.json` and one PNG)

`characters/<id>/sheet.json` and one PNG in the same folder. The PNG is a grid: one row per clip, frames left to right, all the same size, transparent background.

```json
{
  "id": "sprout",
  "displayName": "Sprout",
  "image": "sprout.png",
  "frameWidth": 128,
  "frameHeight": 128,
  "pixelArt": false,
  "clips": {
    "sleep": { "row": 0, "frames": 4, "fps": 4,  "loop": true },
    "work":  { "row": 1, "frames": 6, "fps": 10, "loop": true },
    "wait":  { "row": 2, "frames": 6, "fps": 6,  "loop": true },
    "knock": { "row": 3, "frames": 6, "fps": 12, "loop": true },
    "rest":  { "row": 4, "frames": 4, "fps": 5,  "loop": true }
  }
}
```

This example needs a 768 × 640 PNG (6 frames wide, 5 rows of 128 px). Each clip needs all four fields: `row`, `frames`, `fps`, `loop`. `pixelArt` is optional and defaults to `false`. A `pixelArt` of `true` scales the frames with nearest-neighbour, in whole steps.

The PNG must be 8-bit RGBA, or a palette image of 1, 2, 4 or 8 bits. 16-bit PNGs and RGB PNGs without alpha are rejected. Every frame must fit inside the image.

## Clip names

| Clip | When it plays | Kind |
|---|---|---|
| `sleep` | session idle | plays once, then holds its first frame |
| `work` | session working | plays once, then holds its first frame |
| `wait` | waiting on a helper agent | plays once, then holds its first frame |
| `step_up` | a knock begins (rises 12 units) | once |
| `knock` | needs the user (permission or question) | loops until answered |
| `step_down` | knock answered | once |
| `alert` | session failed | loops |
| `pace` | session seems stuck | loops |
| `present` | session finished; brings out its "done" card | once |
| `rest` | after `present`; holds the card | loops |

Clip names are exact. Any other name is rejected.

- **Sprite sheet, required:** `sleep`, `work`, `wait`, `knock`, `rest`.
- **Sprite sheet, optional, with fallbacks:** `alert` plays `knock`, `pace` plays `wait`, `present` plays `rest`. Without frames, `step_up` and `step_down` show the still pose lifted (the still pose is the first frame of `wait`).
- **Vector rig:** no clip is required. Any clip you leave out is Knock's own. A pack's own clip goes in `clips/<clip>.json` and replaces Knock's clip of the same name for that pack's rigs. Fields: `name`, `loop`, `durationMs`, `tracks`, `swaps`. The code checks every part a clip moves exists in every rig. A sprite sheet pack with a `clips/` folder is rejected.
- With "reduce motion" on, Knock shows one still pose per state (from the format notes; the code does not check this).

## The den (optional)

When more than 5 sessions run, the extra ones wait in a den. A pack may draw its own: `den.svg` at the top of the pack folder.

- The same SVG rules as a rig part, `viewBox="0 0 64 64"`, at most 6 colors.
- One still picture, drawn at the same size as a character.
- Knock draws the counts on top of the middle of the den, so keep that middle area plain. (Guidance from the format notes; the code does not check the layout.)

## Check, pack and test

From the repo folder, after a build (or with `cargo run -p knock-cli --` in place of `target/debug/knock`):

```
target/debug/knock pack check <path to pack folder>
target/debug/knock pack build <path to pack folder>
```

- `pack check` prints `<Name>: valid, N characters` and exits 0, or lists each problem and exits with an error.
- `pack build` runs the same check, then writes `<id>.knockpack` in the folder you run it from. Add `-o <file.knockpack>` to choose the name.
- A `.knockpack` is a plain deflate zip. Files sit at the zip root or inside one top folder. Symlinks, encrypted entries and unsafe names are rejected. Hidden files such as `.DS_Store` are rejected too.

To test it in Knock: open Settings, go to the Characters section, click "Choose characters…", then "Add a pack…" and pick the file. You can also drop the file on the Settings window or double-click it. Check the preview, then "Add and use it". An invalid pack shows "Why it can't be used" with the reasons.

## Limits and rejections

All numbers are from `packs.rs` and `knock-core`.

**Whole pack:** at most 300 files; 16,777,216 bytes (16 MB) in total, and the `.knockpack` file itself at most 16 MB; 1 to 16 characters, each id unique; all sprite sheets together at most 16,777,216 pixels (16 × 1024 × 1024).

**Each file:** `.json`, `.svg`, `README.md`, `LICENSE` and `den.svg` at most 262,144 bytes (256 KB); each `.png` at most 4,194,304 bytes (4 MB) and 4096 pixels on each side. Names are 1 to 64 characters of `A-Z a-z 0-9 . - _`, not starting with a dot. Ids are 1 to 64 characters of `a-z 0-9 - _`, starting with a letter or digit.

**pack.json:** `format` must be `1`; `displayName` 1 to 64 characters; `author` and `license` at most 128 each; `description` at most 1024.

**Sprite sheets:** `frameWidth` and `frameHeight` 8 to 512 pixels; `frames` 1 to 64; `fps` above 0 and at most 60; clip names from the list; every frame inside the PNG.

**Clips (vector rigs):** `durationMs` above 0 and at most 60000; at most 256 keyframes per track and 64 swaps; keyframe `t` from 0 to `durationMs`, increasing; `x` and `y` from -256 to 256, `rot` from -3600 to 3600, `scaleX` and `scaleY` from 0 to 16, `alpha` from 0 to 1; a swap needs `0 <= start < end <= durationMs`.

**Rigs:** `viewBox` `[0, 0, 64, 64]`; `pivot` and `z` plain numbers from -1024 to 1024; exactly the 10 parts, each once; at most 6 colors per character over all its parts (`none` doesn't count).

**Den:** at most 6 colors.

**SVG (rig parts and den):**
- Elements: `svg g path rect circle ellipse line polyline polygon`.
- Attributes: `xmlns viewBox aria-hidden d x y x1 y1 x2 y2 cx cy r rx ry width height points fill fill-opacity fill-rule stroke stroke-width stroke-opacity stroke-linecap stroke-linejoin stroke-miterlimit stroke-dasharray stroke-dashoffset opacity transform`, each at most once per element.
- Colors: `none` or hex (`#rgb`, `#rrggbb`) only. `xmlns` only on `<svg>`, and only `http://www.w3.org/2000/svg`. Root `<svg>` with `viewBox="0 0 64 64"`.
- Rejected: `&` anywhere, comments, `<!`, `<?` (except one `<?xml` at the very start), text between tags, anything after `</svg>`, `style`, `id`, `class`, `href`, `src`, any `on…` attribute, nesting deeper than 16, `stop-color` (no gradients).

**Files:** anything not in the layout, any extra file in a character folder, any folder other than `clips/` and `characters/<id>/`, and any link.

## Licensing and content

- Your art must be yours, or licensed so Knock can ship it. The code does not check who owns the art or what the license says. That part is up to you.
- Put `author` and `license` in `pack.json`. Settings shows them under the pack's name. A `LICENSE` file is optional. The built-in packs use `CC0-1.0`.
- Recommended: `CC0-1.0` or `CC-BY-4.0` (from the format notes).
- Original designs only. No look-alikes of existing characters, franchises, mascots or logos.
- Kid-friendly. People-like characters are fully clothed and drawn cute.
- A worked example: `art/packs/chibi-girls/` (vector rigs) and `art/packs/fairies/` (sprite sheets).
