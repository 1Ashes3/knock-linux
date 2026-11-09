//! Sprite packs (ADR 0038): the `.knockpack` format and its checks. Pure:
//! the app reads a pack's files and passes them in, so every rule is tested
//! here without a disk. Packs come from strangers, so the checks are strict:
//! a pack is a known set of files in known places, with size limits, and its
//! SVGs are flat shapes with nothing that can run or reach outside.
//!
//! ```text
//! pack.json                     manifest (Manifest)
//! README.md, LICENSE[.txt|.md]  optional, never shown as art
//! clips/<clip>.json             optional: replaces a shared clip for this pack's rigs
//! characters/<id>/rig.json      a vector rig (§8.7, ADR 0017) and its part SVGs
//! characters/<id>/sheet.json    or a sprite sheet (Sheet) and its PNG
//! den.svg                       optional: the overflow den, in the pack's theme
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The `pack.json` `format` this build reads.
pub const FORMAT: u32 = 1;

/// Every rig has these parts (§8.7).
pub const REQUIRED_PARTS: [&str; 10] = [
    "body",
    "head",
    "ear_l",
    "ear_r",
    "eyes_open",
    "eyes_closed",
    "paw_l",
    "paw_r",
    "tail",
    "card",
];

/// The clip names the character controller plays (§8.7). A pack may replace
/// any of them, but can't add new ones: nothing would play them.
pub const CLIP_NAMES: [&str; 10] = [
    "sleep",
    "work",
    "wait",
    "step_up",
    "step_down",
    "knock",
    "alert",
    "pace",
    "present",
    "rest",
];

/// A sprite sheet must draw these; the rest fall back (ADR 0038).
pub const SHEET_REQUIRED: [&str; 5] = ["sleep", "work", "wait", "knock", "rest"];

/// Keyframe fields (§8.7).
pub const TRACK_FIELDS: [&str; 6] = ["x", "y", "rot", "scaleX", "scaleY", "alpha"];

pub const VIEW_BOX: [f64; 4] = [0.0, 0.0, 64.0, 64.0];
pub const MAX_COLORS: usize = 6;

/// Limits that keep a pack small and its checks cheap.
pub const MAX_FILES: usize = 300;
pub const MAX_PACK_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
pub const MAX_PNG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_IMAGE_SIDE: u32 = 4096;
pub const MAX_CHARACTERS: usize = 16;
pub const MAX_FRAMES: u32 = 64;
/// Clip limits (security review finding 6).
pub const MAX_CLIP_MS: f64 = 60_000.0;
pub const MAX_KEYFRAMES: usize = 256;
pub const MAX_SWAPS: usize = 64;

/// The range a keyframe field may take.
fn field_range(field: &str) -> Option<(f64, f64)> {
    match field {
        "x" | "y" => Some((-256.0, 256.0)),
        "rot" => Some((-3600.0, 3600.0)),
        "scaleX" | "scaleY" => Some((0.0, 16.0)),
        "alpha" => Some((0.0, 1.0)),
        _ => None,
    }
}

/// All sprite sheets together, in pixels: 64 MB once decoded.
pub const MAX_PACK_PIXELS: u64 = 16 * 1024 * 1024;

/// A pack's files: path relative to the pack folder with `/` separators →
/// contents.
pub type PackFiles = BTreeMap<String, Vec<u8>>;

/// `pack.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Character ids, in the order sessions get them.
    pub characters: Vec<String>,
}

/// `characters/<id>/sheet.json`: frames in a grid, one row per clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sheet {
    pub id: String,
    pub display_name: String,
    /// The PNG, in the character's folder.
    pub image: String,
    pub frame_width: u32,
    pub frame_height: u32,
    /// Scaled with nearest-neighbour and to whole multiples.
    #[serde(default)]
    pub pixel_art: bool,
    pub clips: BTreeMap<String, SheetClip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetClip {
    pub row: u32,
    pub frames: u32,
    pub fps: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Rig,
    Sheet,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterInfo {
    pub id: String,
    pub display_name: String,
    pub kind: Kind,
}

/// The result of [`check_pack`]. Usable only when [`PackCheck::ok`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PackCheck {
    pub manifest: Option<Manifest>,
    pub characters: Vec<CharacterInfo>,
    /// The pack draws its own overflow den (`den.svg`), checked like a part.
    pub den: bool,
    /// Plain-language problems, each naming the file. Never pack content.
    pub errors: Vec<String>,
}

impl PackCheck {
    pub fn ok(&self) -> bool {
        self.errors.is_empty() && self.manifest.is_some()
    }
}

/// Pack and character ids: 1-64 of `a-z 0-9 - _`, starting with a letter or
/// digit. They become folder names and the §8.6 animal ids.
pub fn is_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

/// A file name inside a pack: 1-64 of `A-Z a-z 0-9 . - _`, not hidden.
fn is_file_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('.')
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}

fn ext(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, e)| e)
}

/// Where a file may be, by its path.
#[derive(Debug, PartialEq)]
enum Place<'a> {
    Manifest,
    Doc,
    Den,
    Clip(&'a str),
    Character(&'a str, &'a str),
}

fn place(path: &str) -> Option<Place<'_>> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["pack.json"] => Some(Place::Manifest),
        ["den.svg"] => Some(Place::Den),
        ["README.md" | "LICENSE" | "LICENSE.txt" | "LICENSE.md"] => Some(Place::Doc),
        ["clips", f] if is_file_name(f) && ext(f) == "json" => {
            Some(Place::Clip(f.trim_end_matches(".json")))
        }
        ["characters", id, f] if is_id(id) && is_file_name(f) => {
            matches!(ext(f), "json" | "svg" | "png").then_some(Place::Character(id, f))
        }
        _ => None,
    }
}

/// Every problem with `files`; see [`PackCheck`]. Stops early when the pack
/// is too big to check at all.
pub fn check_pack(files: &PackFiles) -> PackCheck {
    let mut out = PackCheck::default();
    let errors = &mut out.errors;
    if files.len() > MAX_FILES {
        errors.push(format!(
            "the pack has {} files, at most {MAX_FILES} allowed",
            files.len()
        ));
        return out;
    }
    let total: u64 = files.values().map(|b| b.len() as u64).sum();
    if total > MAX_PACK_BYTES {
        errors.push(format!(
            "the pack is {} MB, at most {} MB allowed",
            total / (1024 * 1024),
            MAX_PACK_BYTES / (1024 * 1024)
        ));
        return out;
    }
    for (path, bytes) in files {
        let Some(p) = place(path) else {
            errors.push(format!("{path}: not a file a pack may contain"));
            continue;
        };
        let limit = match p {
            Place::Character(_, f) if ext(f) == "png" => MAX_PNG_BYTES,
            _ => MAX_TEXT_BYTES,
        };
        if bytes.len() > limit {
            errors.push(format!("{path}: larger than {} KB", limit / 1024));
        }
    }
    if !errors.is_empty() {
        return out;
    }

    let Some(manifest) = manifest(files, errors) else {
        return out;
    };
    let listed: BTreeSet<&str> = manifest.characters.iter().map(String::as_str).collect();
    let folders: BTreeSet<&str> = (files.keys())
        .filter_map(|p| match place(p) {
            Some(Place::Character(id, _)) => Some(id),
            _ => None,
        })
        .collect();
    for id in folders.difference(&listed) {
        errors.push(format!("characters/{id}: not listed in pack.json"));
    }

    let mut rig_parts: Vec<(String, BTreeSet<String>)> = Vec::new();
    // Every file a character uses, and the sheets' total pixels.
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut pixels: u64 = 0;
    for id in &manifest.characters {
        let rig = files.get(&format!("characters/{id}/rig.json"));
        let sheet = files.get(&format!("characters/{id}/sheet.json"));
        let info = match (rig, sheet) {
            (Some(_), Some(_)) => {
                errors.push(format!("characters/{id}: has both rig.json and sheet.json"));
                continue;
            }
            (None, None) => {
                errors.push(format!("characters/{id}: needs rig.json or sheet.json"));
                continue;
            }
            (Some(bytes), None) => {
                check_rig(id, bytes, files, &mut used, errors).map(|(name, parts)| {
                    rig_parts.push((id.clone(), parts));
                    (name, Kind::Rig)
                })
            }
            (None, Some(bytes)) => check_sheet(id, bytes, files, &mut used, &mut pixels, errors)
                .map(|n| (n, Kind::Sheet)),
        };
        if let Some((display_name, kind)) = info {
            out.characters.push(CharacterInfo {
                id: id.clone(),
                display_name,
                kind,
            });
        }
    }

    // Security review finding 2: a file nobody uses is never checked, so
    // it may not be in the pack at all.
    for path in files.keys() {
        if matches!(place(path), Some(Place::Character(..))) && !used.contains(path) {
            errors.push(format!("{path}: not used by its character; remove it"));
        }
    }
    // The den: one flat SVG on the 64-unit canvas, like a rig part.
    if let Some(svg) = files.get("den.svg") {
        match std::str::from_utf8(svg)
            .map_err(|_| "is not UTF-8 text".to_string())
            .and_then(check_svg)
        {
            Ok(c) if c.iter().collect::<BTreeSet<_>>().len() > MAX_COLORS => errors.push(format!(
                "den.svg: uses {} colors, at most {MAX_COLORS} allowed",
                c.iter().collect::<BTreeSet<_>>().len()
            )),
            Ok(_) => out.den = true,
            Err(why) => errors.push(format!("den.svg: {why}")),
        }
    }
    // Finding 4: decoded images take 4 bytes a pixel, whatever the PNG's size.
    if pixels > MAX_PACK_PIXELS {
        errors.push(format!(
            "the sprite sheets add up to {} megapixels, at most {} allowed",
            pixels / 1_000_000,
            MAX_PACK_PIXELS / 1_000_000
        ));
    }

    for (path, bytes) in files {
        let Some(Place::Clip(name)) = place(path) else {
            continue;
        };
        let has_rigs = (manifest.characters.iter())
            .any(|id| files.contains_key(&format!("characters/{id}/rig.json")));
        if !has_rigs {
            errors.push(format!(
                "{path}: clips move rig parts, and this pack has no rigs"
            ));
            continue;
        }
        // A rig with problems was reported already; its parts can't be
        // trusted, so the clips are checked against the good rigs only.
        if rig_parts.is_empty() {
            continue;
        }
        match serde_json::from_slice::<Value>(bytes) {
            Ok(v) => check_clip(path, name, &v, &rig_parts, errors),
            Err(_) => errors.push(format!("{path}: not valid JSON")),
        }
    }
    out.manifest = Some(manifest);
    out
}

fn manifest(files: &PackFiles, errors: &mut Vec<String>) -> Option<Manifest> {
    let Some(bytes) = files.get("pack.json") else {
        errors.push("pack.json is missing".into());
        return None;
    };
    let m: Manifest = match serde_json::from_slice(bytes) {
        Ok(m) => m,
        Err(e) => {
            // serde's message names the field, never the content.
            errors.push(format!(
                "pack.json: {}",
                e.to_string().split(" at line").next().unwrap_or("invalid")
            ));
            return None;
        }
    };
    let before = errors.len();
    if m.format != FORMAT {
        errors.push(format!(
            "pack.json: format {} is not supported (this Knock reads {FORMAT})",
            m.format
        ));
    }
    if !is_id(&m.id) {
        errors.push("pack.json: id must be 1-64 of a-z, 0-9, - and _".into());
    }
    if m.id == "classic" {
        errors.push("pack.json: id \"classic\" is Knock's own pack".into());
    }
    if !(1..=64).contains(&m.display_name.trim().len()) {
        errors.push("pack.json: displayName must be 1-64 characters".into());
    }
    for (field, v) in [("author", &m.author), ("license", &m.license)] {
        if v.as_ref().is_some_and(|s| s.len() > 128) {
            errors.push(format!("pack.json: {field} is longer than 128 characters"));
        }
    }
    if m.description.as_ref().is_some_and(|s| s.len() > 1024) {
        errors.push("pack.json: description is longer than 1024 characters".into());
    }
    if !(1..=MAX_CHARACTERS).contains(&m.characters.len()) {
        errors.push(format!(
            "pack.json: characters must list 1-{MAX_CHARACTERS} ids"
        ));
    }
    let mut seen = BTreeSet::new();
    for c in &m.characters {
        if !is_id(c) {
            errors.push(format!(
                "pack.json: character id {c:?} must be 1-64 of a-z, 0-9, - and _"
            ));
        } else if !seen.insert(c) {
            errors.push(format!("pack.json: character {c} is listed twice"));
        }
    }
    (errors.len() == before).then_some(m)
}

fn is_num(v: &Value) -> bool {
    v.as_f64().is_some_and(f64::is_finite)
}

/// `rig.json` (§8.7). Typed, so unknown fields are refused (security
/// review finding 3).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RigFile {
    id: String,
    display_name: String,
    view_box: [f64; 4],
    parts: Vec<RigPartFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RigPartFile {
    name: String,
    file: String,
    pivot: [f64; 2],
    z: f64,
}

/// A rig: its display name and part names, or `None` if it can't be used.
/// It has exactly the 10 [`REQUIRED_PARTS`], each once.
fn check_rig(
    id: &str,
    bytes: &[u8],
    files: &PackFiles,
    used: &mut BTreeSet<String>,
    errors: &mut Vec<String>,
) -> Option<(String, BTreeSet<String>)> {
    let path = format!("characters/{id}/rig.json");
    used.insert(path.clone());
    let before = errors.len();
    let rig: RigFile = match serde_json::from_slice(bytes) {
        Ok(r) => r,
        Err(e) => {
            errors.push(format!("{path}: {}", serde_message(&e)));
            return None;
        }
    };
    if rig.id != id {
        errors.push(format!("{path}: id must be \"{id}\""));
    }
    if !(1..=64).contains(&rig.display_name.trim().len()) {
        errors.push(format!("{path}: displayName must be 1-64 characters"));
    }
    if rig.view_box != VIEW_BOX {
        errors.push(format!("{path}: viewBox must be [0, 0, 64, 64]"));
    }
    let mut names = BTreeSet::new();
    let mut colors = BTreeSet::new();
    for p in &rig.parts {
        let at = format!("characters/{id} part {}", p.name);
        if !REQUIRED_PARTS.contains(&p.name.as_str()) {
            errors.push(format!(
                "{path}: part {:?} isn't one of {}",
                p.name,
                REQUIRED_PARTS.join(", ")
            ));
            continue;
        }
        if !names.insert(p.name.clone()) {
            errors.push(format!("{at}: listed twice"));
        }
        if !p
            .pivot
            .iter()
            .chain([&p.z])
            .all(|n| n.is_finite() && n.abs() <= 1024.0)
        {
            errors.push(format!("{at}: pivot and z must be plain numbers"));
        }
        if !is_file_name(&p.file) || ext(&p.file) != "svg" {
            errors.push(format!(
                "{at}: file must be an .svg file in the character's folder"
            ));
            continue;
        }
        let file = format!("characters/{id}/{}", p.file);
        used.insert(file.clone());
        let Some(svg) = files.get(&file) else {
            errors.push(format!("{at}: file {} is missing", p.file));
            continue;
        };
        match std::str::from_utf8(svg)
            .map_err(|_| "is not UTF-8 text".to_string())
            .and_then(check_svg)
        {
            Ok(c) => colors.extend(c),
            Err(why) => errors.push(format!("{file}: {why}")),
        }
    }
    for r in REQUIRED_PARTS {
        if !names.contains(r) {
            errors.push(format!("characters/{id}: missing required part {r}"));
        }
    }
    if colors.len() > MAX_COLORS {
        errors.push(format!(
            "characters/{id}: uses {} colors, at most {MAX_COLORS} allowed",
            colors.len()
        ));
    }
    (errors.len() == before).then(|| (rig.display_name.trim().to_string(), names))
}

/// serde's message, which names the field, without the line and column.
fn serde_message(e: &serde_json::Error) -> String {
    e.to_string()
        .split(" at line")
        .next()
        .unwrap_or("invalid")
        .to_string()
}

/// The SVG namespace, the only one a part SVG may name.
const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// The elements a part SVG may use: plain shapes (§8.7, ADR 0038).
pub const SVG_ELEMENTS: [&str; 9] = [
    "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];

/// The attributes a part SVG may use: geometry, paint and `transform`.
/// Nothing that names a resource (`href`, `src`, `style`, `class`, `id`,
/// `filter`, `mask`, `clip-path`…) and no `on…` handlers.
pub const SVG_ATTRIBUTES: [&str; 31] = [
    "xmlns",
    "viewBox",
    "aria-hidden",
    "d",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "width",
    "height",
    "points",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "opacity",
    "transform",
];

/// Deepest element nesting a part may use.
const MAX_SVG_DEPTH: usize = 16;

/// Checks one part SVG and returns its colors (`fill` and `stroke`).
///
/// An allowlist, not a blocklist (security review finding 1): the file is
/// read as a strict subset of XML, so nothing can hide behind namespace
/// prefixes, character references or attributes a blocklist forgot.
/// - Only an optional `<?xml …?>` declaration, then one `<svg>` with
///   `viewBox="0 0 64 64"`; whitespace between tags, never text.
/// - Elements from [`SVG_ELEMENTS`], attributes from [`SVG_ATTRIBUTES`],
///   each once, no prefixes (`:`); `xmlns` only on `<svg>`, only the SVG
///   namespace.
/// - No `&` anywhere (entities, character references), no `<!` (comments,
///   CDATA, DOCTYPE) and no other `<?`.
/// - Colors are `none` or hex (`#rgb`, `#rrggbb`); other values are plain
///   numbers, words and path data.
pub fn check_svg(svg: &str) -> Result<Vec<String>, String> {
    if svg.contains('&') {
        return Err("contains &; entities and character references aren't allowed".into());
    }
    let mut rest = svg.trim();
    if let Some(after) = rest.strip_prefix("<?xml") {
        let (decl, after) = after.split_once("?>").ok_or("is not an SVG file")?;
        if decl.contains('<') {
            return Err("is not an SVG file".into());
        }
        rest = after.trim_start();
    }
    let mut colors = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    let (mut saw_root, mut view_box) = (false, false);
    loop {
        let Some(i) = rest.find('<') else {
            if !rest.trim().is_empty() {
                return Err("has text outside the shapes; only shapes are allowed".into());
            }
            break;
        };
        if !rest[..i].trim().is_empty() {
            return Err("has text inside the drawing; only shapes are allowed".into());
        }
        if saw_root && open.is_empty() {
            return Err("has something after </svg>".into());
        }
        rest = &rest[i + 1..];
        if rest.starts_with('!') || rest.starts_with('?') {
            return Err("contains a comment, CDATA, DOCTYPE or instruction (<! or <?)".into());
        }
        let end = rest.find('>').ok_or("has an unfinished tag")?;
        let (tag, after) = (&rest[..end], &rest[end + 1..]);
        rest = after;
        if let Some(name) = tag.strip_prefix('/') {
            if open.pop() != Some(name.trim()) {
                return Err("has tags that don't match".into());
            }
            continue;
        }
        let (tag, closed) = match tag.strip_suffix('/') {
            Some(t) => (t, true),
            None => (tag, false),
        };
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        if !SVG_ELEMENTS.contains(&name) {
            return Err(format!(
                "uses <{name}>; only {} are allowed",
                SVG_ELEMENTS.join(", ")
            ));
        }
        match (saw_root, name) {
            (false, "svg") => saw_root = true,
            (false, _) => return Err("is not an SVG file".into()),
            (true, "svg") => return Err("has an <svg> inside the <svg>".into()),
            _ => {}
        }
        let mut seen: Vec<&str> = Vec::new();
        let mut attrs = &tag[name_end..];
        loop {
            attrs = attrs.trim_start();
            if attrs.is_empty() {
                break;
            }
            let eq = attrs
                .find('=')
                .ok_or(format!("<{name}> has an attribute without a value"))?;
            let attr = attrs[..eq].trim_end();
            if !SVG_ATTRIBUTES.contains(&attr) {
                return Err(format!("uses the attribute {attr:?}, which isn't allowed"));
            }
            if seen.contains(&attr) {
                return Err(format!("<{name}> sets {attr} twice"));
            }
            seen.push(attr);
            let v = attrs[eq + 1..].trim_start();
            let quote = v.chars().next().filter(|c| *c == '"' || *c == '\'');
            let quote = quote.ok_or(format!("{attr} needs a quoted value"))?;
            let close = v[1..]
                .find(quote)
                .ok_or(format!("{attr} has an unfinished value"))?;
            let value = &v[1..1 + close];
            attrs = &v[close + 2..];
            check_value(name, attr, value, &mut colors, &mut view_box)?;
        }
        if !closed {
            if open.len() >= MAX_SVG_DEPTH {
                return Err("nests elements too deeply".into());
            }
            open.push(name);
        }
    }
    if !saw_root || !open.is_empty() {
        return Err("is not a complete SVG file".into());
    }
    if !view_box {
        return Err(r#"must use viewBox="0 0 64 64""#.into());
    }
    Ok(colors)
}

/// One attribute's value: see [`check_svg`].
fn check_value(
    element: &str,
    attr: &str,
    value: &str,
    colors: &mut Vec<String>,
    view_box: &mut bool,
) -> Result<(), String> {
    match attr {
        "xmlns" if element == "svg" && value == SVG_NS => return Ok(()),
        "xmlns" => return Err(format!("xmlns must be {SVG_NS:?}, on <svg> only")),
        "viewBox" if element == "svg" => {
            let parts: Vec<&str> = value.split([' ', ',']).filter(|p| !p.is_empty()).collect();
            if parts != ["0", "0", "64", "64"] {
                return Err(r#"must use viewBox="0 0 64 64""#.into());
            }
            *view_box = true;
            return Ok(());
        }
        "fill" | "stroke" => {
            let v = value.trim().to_ascii_lowercase();
            let hex = |h: &str| {
                (h.len() == 3 || h.len() == 6) && h.bytes().all(|b| b.is_ascii_hexdigit())
            };
            match v.strip_prefix('#') {
                _ if v == "none" => {}
                Some(h) if hex(h) => colors.push(if h.len() == 3 {
                    h.chars()
                        .flat_map(|c| [c, c])
                        .fold(String::from("#"), |mut s, c| {
                            s.push(c);
                            s
                        })
                } else {
                    v.clone()
                }),
                _ => return Err(format!("{attr} must be none or a hex color like #e0702a")),
            }
            return Ok(());
        }
        _ => {}
    }
    // Numbers, words (round, nonzero, true) and path data; `transform`
    // also needs its parentheses.
    let extra = if attr == "transform" { "()" } else { "" };
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " .,+-%\t\n\r".contains(c) || extra.contains(c));
    if !plain {
        return Err(format!("{attr} has characters that aren't allowed"));
    }
    Ok(())
}

/// Reads the pack folder `dir` into [`PackFiles`] without trusting it
/// (security review finding 5): only the folders and file names a pack may
/// have ([`place`]), checked before anything is read; no links followed (one
/// could point at `~/.ssh`); only regular files, opened without following a
/// link or blocking on a pipe and checked again once open; and reading stops
/// as soon as the file count or total size passes the limits. [`check_pack`]
/// then checks the rest. Errors name the file, never its content.
///
/// Left open: another process running as the same user could swap a folder
/// for a link between listing and reading it. That process could read the
/// user's files anyway.
#[cfg(feature = "fs")]
pub fn read_folder(dir: &std::path::Path) -> Result<PackFiles, String> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| format!("can't open the pack ({})", e.kind()))?;
    if !meta.is_dir() {
        return Err("the pack is not a folder".into());
    }
    let mut files = PackFiles::new();
    let mut total: u64 = 0;
    let mut todo = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = todo.pop() {
        let entries =
            std::fs::read_dir(&path).map_err(|e| format!("{prefix}: can't read ({})", e.kind()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{prefix}: can't read ({})", e.kind()))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| format!("{prefix}: a file name is not valid text"))?;
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            // `file_type` doesn't follow links, so a link is seen as a link.
            let kind = entry
                .file_type()
                .map_err(|e| format!("{rel}: can't read ({})", e.kind()))?;
            if kind.is_symlink() {
                return Err(format!("{rel}: links are not allowed in a pack"));
            }
            if kind.is_dir() {
                if !pack_folder(&rel) {
                    return Err(format!("{rel}: not a folder a pack may contain"));
                }
                todo.push((entry.path(), rel));
                continue;
            }
            if !kind.is_file() {
                return Err(format!("{rel}: not a regular file"));
            }
            // Checked before reading, so a big file with a wrong name is
            // never read.
            if place(&rel).is_none() {
                return Err(format!("{rel}: not a file a pack may contain"));
            }
            if files.len() >= MAX_FILES {
                return Err(format!("the pack has more than {MAX_FILES} files"));
            }
            let left = MAX_PACK_BYTES - total;
            let mut bytes = Vec::new();
            // At most one byte past what's left, so a growing file can't
            // slip past the limit between a size check and the read.
            open_plain(&entry.path())
                .and_then(|f| f.take(left + 1).read_to_end(&mut bytes))
                .map_err(|e| format!("{rel}: can't read ({})", e.kind()))?;
            total += bytes.len() as u64;
            if total > MAX_PACK_BYTES {
                return Err(format!(
                    "the pack is larger than {} MB",
                    MAX_PACK_BYTES / (1024 * 1024)
                ));
            }
            files.insert(rel, bytes);
        }
    }
    Ok(files)
}

/// The folders a pack may have: `clips`, `characters` and
/// `characters/<id>`.
#[cfg(feature = "fs")]
fn pack_folder(rel: &str) -> bool {
    match rel.split('/').collect::<Vec<_>>().as_slice() {
        ["clips" | "characters"] => true,
        ["characters", id] => is_id(id),
        _ => false,
    }
}

/// Opens a regular file for reading without following a link at its name
/// or blocking on a pipe (unix), and checks what was opened is a regular
/// file.
#[cfg(feature = "fs")]
fn open_plain(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let f = opts.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(f)
}

/// Reads a `.knockpack` (a zip) into [`PackFiles`] without trusting it,
/// entirely in memory: nothing is written anywhere until [`check_pack`] has
/// passed. Entries that are links, encrypted, use another compression than
/// deflate, or have a name that could escape (`..`, `/…`, `\`, `C:`) are
/// refused. Sizes in the zip's headers aren't trusted: reading stops as soon
/// as the bytes actually decompressed pass [`MAX_PACK_BYTES`], so a zip bomb
/// costs at most that much. A zip whose files sit in one top folder
/// (`fairies/pack.json`) reads as if they didn't.
#[cfg(feature = "zip")]
pub fn read_zip(bytes: &[u8]) -> Result<PackFiles, String> {
    use std::io::Read;
    if bytes.len() as u64 > MAX_PACK_BYTES {
        return Err(format!(
            "the file is larger than {} MB",
            MAX_PACK_BYTES / (1024 * 1024)
        ));
    }
    let not_zip = || "this isn't a .knockpack file (a zip)".to_string();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| not_zip())?;
    if zip.len() > MAX_FILES * 4 {
        return Err(format!("the pack has more than {MAX_FILES} files"));
    }
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| {
            format!(
                "entry {} can't be read (encrypted, damaged or not deflate)",
                i + 1
            )
        })?;
        let name = entry
            .name()
            .map_err(|_| format!("entry {} has a name that isn't valid text", i + 1))?
            .to_string();
        if entry.is_symlink() {
            return Err(format!("{name}: links are not allowed in a pack"));
        }
        if entry.is_dir() {
            continue;
        }
        if !safe_entry_name(&name) {
            return Err(format!("{name}: not a safe file name"));
        }
        if entries.len() >= MAX_FILES {
            return Err(format!("the pack has more than {MAX_FILES} files"));
        }
        let left = MAX_PACK_BYTES - total;
        let mut data = Vec::new();
        (&mut entry)
            .take(left + 1)
            .read_to_end(&mut data)
            .map_err(|_| format!("{name}: can't be unpacked"))?;
        total += data.len() as u64;
        if total > MAX_PACK_BYTES {
            return Err(format!(
                "the pack unpacks to more than {} MB",
                MAX_PACK_BYTES / (1024 * 1024)
            ));
        }
        entries.push((name, data));
    }
    // One top folder around everything: drop it.
    let top = (!entries.iter().any(|(n, _)| n == "pack.json"))
        .then(|| {
            entries
                .first()
                .and_then(|(n, _)| n.split_once('/'))
                .map(|(t, _)| format!("{t}/"))
        })
        .flatten()
        .filter(|t| entries.iter().all(|(n, _)| n.starts_with(t.as_str())));
    let mut files = PackFiles::new();
    for (name, data) in entries {
        let rel = match &top {
            Some(t) => name[t.len()..].to_string(),
            None => name,
        };
        if files.insert(rel.clone(), data).is_some() {
            return Err(format!("{rel}: in the pack twice"));
        }
    }
    Ok(files)
}

/// A zip entry name a pack may use: relative, `/`-separated, no `.`/`..`
/// or empty parts, no `\` or `:` (Windows paths and drives), plain text.
#[cfg(feature = "zip")]
fn safe_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name.chars().all(|c| !c.is_control())
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// A `.knockpack` of `files`, for artists (`knock pack build`): deflated,
/// in path order, plain 0644 files.
#[cfg(feature = "zip")]
pub fn write_zip(files: &PackFiles) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, bytes) in files {
        zip.start_file(path.as_str(), options)
            .and_then(|()| zip.write_all(bytes).map_err(Into::into))
            .map_err(|e| format!("{path}: can't be added ({e})"))?;
    }
    let out = zip
        .finish()
        .map_err(|e| format!("can't finish the file ({e})"))?;
    Ok(out.into_inner())
}

/// A PNG's width and height, or `None` unless it is a well-formed PNG a
/// sheet may use (security review finding 4): a 13-byte `IHDR` first, 8-bit
/// RGBA or a palette of up to 8 bits, standard compression and filter, and
/// the `IEND` chunk last. Pixel data isn't decoded here; the limits on the
/// image's sides and the pack's total pixels bound what decoding can cost.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82];
    if bytes.len() < 8 + 25 + 12
        || bytes[..8] != SIGNATURE
        || bytes[8..12] != [0, 0, 0, 13]
        || &bytes[12..16] != b"IHDR"
        || !bytes.ends_with(&IEND)
    {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (depth, color, compression, filter, interlace) =
        (bytes[24], bytes[25], bytes[26], bytes[27], bytes[28]);
    let pixels_ok = matches!((color, depth), (6, 8) | (3, 1 | 2 | 4 | 8));
    (pixels_ok && compression == 0 && filter == 0 && interlace <= 1).then(|| (be(16), be(20)))
}

fn check_sheet(
    id: &str,
    bytes: &[u8],
    files: &PackFiles,
    used: &mut BTreeSet<String>,
    pixels: &mut u64,
    errors: &mut Vec<String>,
) -> Option<String> {
    let path = format!("characters/{id}/sheet.json");
    used.insert(path.clone());
    let before = errors.len();
    let sheet: Sheet = match serde_json::from_slice(bytes) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!(
                "{path}: {}",
                e.to_string().split(" at line").next().unwrap_or("invalid")
            ));
            return None;
        }
    };
    if sheet.id != id {
        errors.push(format!("{path}: id must be \"{id}\""));
    }
    if !(1..=64).contains(&sheet.display_name.trim().len()) {
        errors.push(format!("{path}: displayName must be 1-64 characters"));
    }
    for (side, n) in [
        ("frameWidth", sheet.frame_width),
        ("frameHeight", sheet.frame_height),
    ] {
        if !(8..=512).contains(&n) {
            errors.push(format!("{path}: {side} must be 8-512 pixels"));
        }
    }
    let size = if !is_file_name(&sheet.image) || ext(&sheet.image) != "png" {
        errors.push(format!(
            "{path}: image must be a .png file in the character's folder"
        ));
        None
    } else {
        used.insert(format!("characters/{id}/{}", sheet.image));
        match files.get(&format!("characters/{id}/{}", sheet.image)) {
            None => {
                errors.push(format!("{path}: image {} is missing", sheet.image));
                None
            }
            Some(png) => match png_size(png) {
                None => {
                    errors.push(format!(
                        "characters/{id}/{}: is not a PNG image Knock can use (8-bit RGBA or a palette)",
                        sheet.image
                    ));
                    None
                }
                Some((w, h)) if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE || w == 0 || h == 0 => {
                    errors.push(format!(
                        "characters/{id}/{}: must be at most {MAX_IMAGE_SIDE}×{MAX_IMAGE_SIDE} pixels",
                        sheet.image
                    ));
                    None
                }
                Some((w, h)) => {
                    *pixels += u64::from(w) * u64::from(h);
                    Some((w, h))
                }
            },
        }
    };
    for (name, clip) in &sheet.clips {
        let at = format!("{path}: clip {name}");
        if !CLIP_NAMES.contains(&name.as_str()) {
            errors.push(format!(
                "{at}: not a clip Knock plays ({})",
                CLIP_NAMES.join(", ")
            ));
            continue;
        }
        if !(1..=MAX_FRAMES).contains(&clip.frames) {
            errors.push(format!("{at}: frames must be 1-{MAX_FRAMES}"));
        }
        if !(clip.fps.is_finite() && clip.fps > 0.0 && clip.fps <= 60.0) {
            errors.push(format!("{at}: fps must be above 0 and at most 60"));
        }
        if let Some((w, h)) = size {
            let right = u64::from(clip.frames) * u64::from(sheet.frame_width);
            let bottom = (u64::from(clip.row) + 1) * u64::from(sheet.frame_height);
            if right > u64::from(w) || bottom > u64::from(h) {
                errors.push(format!(
                    "{at}: its frames run past the edge of {}",
                    sheet.image
                ));
            }
        }
    }
    for r in SHEET_REQUIRED {
        if !sheet.clips.contains_key(r) {
            errors.push(format!("{path}: missing required clip {r}"));
        }
    }
    (errors.len() == before).then(|| sheet.display_name.trim().to_string())
}

/// One clip (§8.7, ADR 0017), as `art:check` checks the shared ones. Every
/// part it moves must exist in every rig of the pack.
pub fn check_clip(
    path: &str,
    file_name: &str,
    clip: &Value,
    rigs: &[(String, BTreeSet<String>)],
    errors: &mut Vec<String>,
) {
    if !CLIP_NAMES.contains(&file_name) {
        errors.push(format!(
            "{path}: not a clip Knock plays ({})",
            CLIP_NAMES.join(", ")
        ));
        return;
    }
    let Some(c) = clip.as_object() else {
        errors.push(format!("{path}: must be an object"));
        return;
    };
    for k in c.keys() {
        if !["name", "loop", "durationMs", "tracks", "swaps"].contains(&k.as_str()) {
            errors.push(format!("{path}: unknown field {k}"));
        }
    }
    if c.get("name").and_then(Value::as_str) != Some(file_name) {
        errors.push(format!("{path}: name must be \"{file_name}\""));
    }
    if !c.get("loop").is_some_and(Value::is_boolean) {
        errors.push(format!("{path}: loop must be true or false"));
    }
    let Some(d) = c
        .get("durationMs")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite() && *d > 0.0 && *d <= MAX_CLIP_MS)
    else {
        errors.push(format!(
            "{path}: durationMs must be a positive number, at most {MAX_CLIP_MS}"
        ));
        return;
    };
    let in_clip = |v: Option<&Value>| {
        v.and_then(Value::as_f64)
            .is_some_and(|t| t.is_finite() && (0.0..=d).contains(&t))
    };
    let missing_in = |part: &str| -> Vec<&str> {
        (rigs.iter())
            .filter(|(_, parts)| !parts.contains(part))
            .map(|(id, _)| id.as_str())
            .collect()
    };
    match c.get("tracks").and_then(Value::as_object) {
        None => errors.push(format!("{path}: tracks must be an object")),
        Some(tracks) => {
            for (part, keys) in tracks {
                let at = format!("{path}: track {part}");
                let lacking = missing_in(part);
                if !lacking.is_empty() {
                    errors.push(format!(
                        "{at}: unknown part (not in {})",
                        lacking.join(", ")
                    ));
                }
                let Some(keys) = keys.as_array().filter(|k| !k.is_empty()) else {
                    errors.push(format!("{at}: must be a non-empty list of keyframes"));
                    continue;
                };
                if keys.len() > MAX_KEYFRAMES {
                    errors.push(format!("{at}: at most {MAX_KEYFRAMES} keyframes"));
                    continue;
                }
                let mut prev = -1.0;
                for k in keys {
                    let Some(k) = k.as_object().filter(|k| in_clip(k.get("t"))) else {
                        errors.push(format!("{at}: every keyframe needs t between 0 and {d}"));
                        continue;
                    };
                    let t = k["t"].as_f64().unwrap_or(0.0);
                    if t <= prev {
                        errors.push(format!("{at}: keyframes must be in increasing t order"));
                    }
                    prev = t;
                    for (f, v) in k {
                        if f == "t" {
                            continue;
                        }
                        if !TRACK_FIELDS.contains(&f.as_str()) {
                            errors.push(format!("{at}: unknown track field {f}"));
                        } else if !is_num(v) {
                            errors.push(format!("{at}: {f} must be a number"));
                        } else if let Some((lo, hi)) = field_range(f)
                            && !(lo..=hi).contains(&v.as_f64().unwrap_or(f64::NAN))
                        {
                            errors.push(format!("{at}: {f} must be from {lo} to {hi}"));
                        }
                    }
                }
            }
        }
    }
    let Some(swaps) = c.get("swaps").and_then(Value::as_array) else {
        errors.push(format!("{path}: swaps must be a list"));
        return;
    };
    if swaps.len() > MAX_SWAPS {
        errors.push(format!("{path}: at most {MAX_SWAPS} swaps"));
        return;
    }
    for s in swaps {
        let Some(s) = s.as_object() else {
            errors.push(format!("{path}: every swap must be an object"));
            continue;
        };
        for k in s.keys() {
            if !["from", "to", "start", "end"].contains(&k.as_str()) {
                errors.push(format!("{path}: unknown swap field {k}"));
            }
        }
        for key in ["from", "to"] {
            match s.get(key).and_then(Value::as_str) {
                None => errors.push(format!("{path}: swap needs from and to part names")),
                Some(part) => {
                    let lacking = missing_in(part);
                    if !lacking.is_empty() {
                        errors.push(format!(
                            "{path}: swap names unknown part {part} (not in {})",
                            lacking.join(", ")
                        ));
                    }
                }
            }
        }
        let (start, end) = (s.get("start"), s.get("end"));
        let ordered = start.and_then(Value::as_f64) < end.and_then(Value::as_f64);
        if !in_clip(start) || !in_clip(end) || !ordered {
            errors.push(format!(
                "{path}: swap needs 0 <= start < end <= durationMs ({d})"
            ));
        }
    }
}
