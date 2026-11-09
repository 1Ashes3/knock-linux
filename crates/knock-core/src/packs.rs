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
    Clip(&'a str),
    Character(&'a str, &'a str),
}

fn place(path: &str) -> Option<Place<'_>> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["pack.json"] => Some(Place::Manifest),
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
            (Some(bytes), None) => check_rig(id, bytes, files, errors).map(|(name, parts)| {
                rig_parts.push((id.clone(), parts));
                (name, Kind::Rig)
            }),
            (None, Some(bytes)) => check_sheet(id, bytes, files, errors).map(|n| (n, Kind::Sheet)),
        };
        if let Some((display_name, kind)) = info {
            out.characters.push(CharacterInfo {
                id: id.clone(),
                display_name,
                kind,
            });
        }
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

/// A rig: its display name and part names, or `None` if it can't be used.
fn check_rig(
    id: &str,
    bytes: &[u8],
    files: &PackFiles,
    errors: &mut Vec<String>,
) -> Option<(String, BTreeSet<String>)> {
    let path = format!("characters/{id}/rig.json");
    let before = errors.len();
    let Ok(rig) = serde_json::from_slice::<Value>(bytes) else {
        errors.push(format!("{path}: not valid JSON"));
        return None;
    };
    let Some(rig) = rig.as_object() else {
        errors.push(format!("{path}: must be an object"));
        return None;
    };
    if rig.get("id").and_then(Value::as_str) != Some(id) {
        errors.push(format!("{path}: id must be \"{id}\""));
    }
    let name = rig.get("displayName").and_then(Value::as_str).unwrap_or("");
    if !(1..=64).contains(&name.trim().len()) {
        errors.push(format!("{path}: displayName must be 1-64 characters"));
    }
    let view_box = rig.get("viewBox").and_then(Value::as_array);
    if view_box.map(|v| v.iter().map(Value::as_f64).collect::<Vec<_>>())
        != Some(VIEW_BOX.iter().map(|&n| Some(n)).collect())
    {
        errors.push(format!("{path}: viewBox must be [0, 0, 64, 64]"));
    }
    let Some(parts) = rig.get("parts").and_then(Value::as_array) else {
        errors.push(format!("{path}: parts must be a list"));
        return None;
    };
    let mut names = BTreeSet::new();
    let mut colors = BTreeSet::new();
    for p in parts {
        let Some(part) = p.get("name").and_then(Value::as_str) else {
            errors.push(format!("{path}: every part needs a name"));
            continue;
        };
        let at = format!("characters/{id} part {part}");
        if !names.insert(part.to_string()) {
            errors.push(format!("{at}: duplicate part"));
        }
        let pivot = p.get("pivot").and_then(Value::as_array);
        if !pivot.is_some_and(|v| v.len() == 2 && v.iter().all(is_num)) {
            errors.push(format!("{at}: pivot must be [x, y]"));
        }
        if !p.get("z").is_some_and(is_num) {
            errors.push(format!("{at}: z must be a number"));
        }
        let file = p.get("file").and_then(Value::as_str).unwrap_or("");
        if !is_file_name(file) || ext(file) != "svg" {
            errors.push(format!(
                "{at}: file must be an .svg file in the character's folder"
            ));
            continue;
        }
        let Some(svg) = files.get(&format!("characters/{id}/{file}")) else {
            errors.push(format!("{at}: file {file} is missing"));
            continue;
        };
        match std::str::from_utf8(svg)
            .map_err(|_| "is not UTF-8 text".to_string())
            .and_then(check_svg)
        {
            Ok(c) => colors.extend(c),
            Err(why) => errors.push(format!("characters/{id}/{file}: {why}")),
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
    (errors.len() == before).then(|| (name.trim().to_string(), names))
}

/// Markup a pack SVG may never contain. Flat shapes only (§8.7), and
/// nothing that runs, loads, links or expands: scripts, event handlers,
/// external or internal references, CSS, entities.
const FORBIDDEN: &[&str] = &[
    "<!doctype",
    "<!entity",
    "<script",
    "<foreignobject",
    "<image",
    "<use",
    "<style",
    "<iframe",
    "<object",
    "<embed",
    "<a ",
    "<a>",
    "<a\n",
    "<a\t",
    "<animate",
    "<set",
    "<lineargradient",
    "<radialgradient",
    "<filter",
    "<pattern",
    "<mask",
    "<clippath",
    "<mpath",
    "<feimage",
    "href",
    "url(",
    "@import",
    "javascript:",
    "data:",
    "<![cdata[",
];

/// Checks one part SVG; returns its colors (`fill`, `stroke`, `stop-color`).
pub fn check_svg(svg: &str) -> Result<Vec<String>, String> {
    let body = svg.trim();
    let after_decl = match body.strip_prefix("<?xml") {
        Some(rest) => rest.split_once("?>").map_or("", |(_, r)| r).trim_start(),
        None => body,
    };
    let opens = after_decl
        .strip_prefix("<svg")
        .is_some_and(|r| r.starts_with(|c: char| c.is_whitespace() || c == '>'));
    if !opens || !body.ends_with("</svg>") {
        return Err("is not an SVG file".into());
    }
    if !svg.contains(r#"viewBox="0 0 64 64""#) {
        return Err(r#"must use viewBox="0 0 64 64""#.into());
    }
    let lower = svg.to_ascii_lowercase();
    if let Some(bad) = FORBIDDEN.iter().find(|f| lower.contains(*f)) {
        return Err(format!(
            "must be flat shapes only; it contains {:?}",
            bad.trim()
        ));
    }
    if has_event_handler(&lower) {
        return Err("must be flat shapes only; it has an event handler (on…=)".into());
    }
    Ok(colors(&lower))
}

/// An attribute like ` onload=` or ` onclick =`.
fn has_event_handler(lower: &str) -> bool {
    let b = lower.as_bytes();
    (1..b.len().saturating_sub(2)).any(|i| {
        if !(b[i - 1].is_ascii_whitespace() || b[i - 1] == b'"' || b[i - 1] == b'\'')
            || &b[i..i + 2] != b"on"
        {
            return false;
        }
        let mut j = i + 2;
        let start = j;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        j > start && j < b.len() && b[j] == b'='
    })
}

/// Color values, normalized: `#abc` becomes `#aabbcc`; `none` and
/// `transparent` don't count.
fn colors(lower: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["fill", "stroke", "stop-color"] {
        let mut rest = lower;
        while let Some(i) = rest.find(key) {
            let before_ok = i == 0
                || !rest.as_bytes()[i - 1].is_ascii_alphanumeric()
                    && rest.as_bytes()[i - 1] != b'-';
            rest = &rest[i + key.len()..];
            let after = rest.trim_start();
            let Some(v) = after.strip_prefix('=').or_else(|| after.strip_prefix(':')) else {
                continue;
            };
            if !before_ok {
                continue;
            }
            let v = v.trim_start().trim_start_matches(['"', '\'']);
            let end = v
                .find(['"', '\'', ';', ' ', '>', '\n', '/'])
                .unwrap_or(v.len());
            let c = &v[..end];
            if c.is_empty() || c == "none" || c == "transparent" {
                continue;
            }
            let c = match c.strip_prefix('#') {
                Some(h) if h.len() == 3 => {
                    h.chars()
                        .flat_map(|x| [x, x])
                        .fold(String::from("#"), |mut s, x| {
                            s.push(x);
                            s
                        })
                }
                _ => c.to_string(),
            };
            out.push(c);
        }
    }
    out
}

/// Reads the pack folder `dir` into [`PackFiles`] without trusting it: no
/// links are followed (one could point at `~/.ssh`), only regular files are
/// read, at most three levels deep, and reading stops as soon as the file
/// count or total size passes the limits. [`check_pack`] then checks the
/// rest. Errors are plain words naming the file, never its content.
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
    let mut todo = vec![(dir.to_path_buf(), String::new(), 0usize)];
    while let Some((path, prefix, depth)) = todo.pop() {
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
                if depth >= 2 {
                    return Err(format!("{rel}: folders nest too deep for a pack"));
                }
                todo.push((entry.path(), rel, depth + 1));
                continue;
            }
            if !kind.is_file() {
                return Err(format!("{rel}: not a regular file"));
            }
            if files.len() >= MAX_FILES {
                return Err(format!("the pack has more than {MAX_FILES} files"));
            }
            let left = MAX_PACK_BYTES - total;
            let mut bytes = Vec::new();
            // At most one byte past what's left, so a growing file can't
            // slip past the limit between a size check and the read.
            std::fs::File::open(entry.path())
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

/// A PNG's width and height from its header, or `None` if it isn't a PNG.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some((be(16), be(20)))
}

fn check_sheet(
    id: &str,
    bytes: &[u8],
    files: &PackFiles,
    errors: &mut Vec<String>,
) -> Option<String> {
    let path = format!("characters/{id}/sheet.json");
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
        match files.get(&format!("characters/{id}/{}", sheet.image)) {
            None => {
                errors.push(format!("{path}: image {} is missing", sheet.image));
                None
            }
            Some(png) => match png_size(png) {
                None => {
                    errors.push(format!(
                        "characters/{id}/{}: is not a PNG image",
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
                Some(size) => Some(size),
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
        .filter(|d| d.is_finite() && *d > 0.0)
    else {
        errors.push(format!("{path}: durationMs must be a positive number"));
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
