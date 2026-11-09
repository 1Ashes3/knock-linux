//! Installed sprite packs (ADR 0038). Each folder in the packs folder is read
//! with [`knock_core::packs::read_folder`] and checked with
//! [`knock_core::packs::check_pack`]; a valid pack's files stay in memory and
//! the webviews get its images from there (`knockpack://<pack>/<path>`), so
//! nothing on disk is read again after the check. "classic" is Knock's own
//! art, bundled in the UI; it is listed here but has no files.

use knock_core::packs::{self, CharacterInfo, Kind, Manifest, PackCheck, PackFiles};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// The built-in pack's id: Knock's fox, rabbit, cat and owl.
pub const CLASSIC: &str = "classic";

/// At most this many problems are listed per pack.
const MAX_ERRORS: usize = 20;

struct Installed {
    /// The folder name.
    folder: String,
    check: PackCheck,
    /// Only for a valid pack.
    files: PackFiles,
}

impl Installed {
    fn id(&self) -> &str {
        self.check.manifest.as_ref().map_or(&self.folder, |m| &m.id)
    }
}

/// One entry of `list_packs` (§8.5 addition, ADR 0038).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub builtin: bool,
    pub valid: bool,
    /// Why it can't be used; empty when valid.
    pub errors: Vec<String>,
    pub characters: Vec<CharacterInfo>,
}

/// One character of `get_pack`: its rig or sheet as written in the pack.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCharacter {
    pub id: String,
    pub display_name: String,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<Value>,
}

/// `get_pack`: what the overlay needs to draw a pack.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackData {
    pub id: String,
    pub characters: Vec<PackCharacter>,
    /// The pack's own clips, by name; they replace the shared ones.
    pub clips: BTreeMap<String, Value>,
}

pub struct Packs {
    dir: Option<PathBuf>,
    installed: RwLock<Vec<Installed>>,
}

impl Packs {
    /// Packs in `dir` (`None`: no packs folder, only classic).
    pub fn new(dir: Option<PathBuf>) -> Self {
        let packs = Packs {
            dir,
            installed: RwLock::new(Vec::new()),
        };
        packs.reload();
        packs
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Reads and checks every pack folder again.
    pub fn reload(&self) {
        let found = self.dir.as_deref().map(scan).unwrap_or_default();
        let valid = found.iter().filter(|p| p.check.ok()).count();
        tracing::info!(packs = found.len(), valid, "sprite packs loaded");
        *self.installed.write().unwrap_or_else(|e| e.into_inner()) = found;
    }

    /// Classic first, then the installed packs by name.
    pub fn list(&self) -> Vec<PackSummary> {
        let installed = self.installed.read().unwrap_or_else(|e| e.into_inner());
        let mut out = vec![classic()];
        out.extend(installed.iter().map(|p| {
            let m = p.check.manifest.as_ref();
            PackSummary {
                id: p.id().to_string(),
                display_name: m.map_or_else(|| p.folder.clone(), |m| m.display_name.clone()),
                author: m.and_then(|m| m.author.clone()),
                license: m.and_then(|m| m.license.clone()),
                description: m.and_then(|m| m.description.clone()),
                builtin: false,
                valid: p.check.ok(),
                errors: p.check.errors.iter().take(MAX_ERRORS).cloned().collect(),
                characters: p.check.characters.clone(),
            }
        }));
        out
    }

    /// The character ids of a valid installed pack, in assignment order.
    pub fn cast(&self, id: &str) -> Option<Vec<String>> {
        let installed = self.installed.read().unwrap_or_else(|e| e.into_inner());
        let p = installed.iter().find(|p| p.id() == id && p.check.ok())?;
        p.check.manifest.as_ref().map(|m| m.characters.clone())
    }

    /// A valid installed pack for the overlay; `None` for classic or an
    /// unknown or invalid pack.
    pub fn get(&self, id: &str) -> Option<PackData> {
        let installed = self.installed.read().unwrap_or_else(|e| e.into_inner());
        let p = installed.iter().find(|p| p.id() == id && p.check.ok())?;
        let json = |path: &str| {
            p.files
                .get(path)
                .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        };
        let characters = (p.check.characters.iter())
            .map(|c| {
                let file = |f: &str| json(&format!("characters/{}/{f}", c.id));
                PackCharacter {
                    id: c.id.clone(),
                    display_name: c.display_name.clone(),
                    kind: c.kind,
                    rig: (c.kind == Kind::Rig).then(|| file("rig.json")).flatten(),
                    sheet: (c.kind == Kind::Sheet)
                        .then(|| file("sheet.json"))
                        .flatten(),
                }
            })
            .collect();
        let clips = (p.files.keys())
            .filter_map(|path| {
                let name = path.strip_prefix("clips/")?.strip_suffix(".json")?;
                Some((name.to_string(), json(path)?))
            })
            .collect();
        Some(PackData {
            id: p.id().to_string(),
            characters,
            clips,
        })
    }

    /// An image of a valid pack for `knockpack://`: only a character's
    /// `.svg` or `.png`, never its JSON or docs. Returns the bytes and their
    /// media type.
    pub fn image(&self, pack: &str, path: &str) -> Option<(Vec<u8>, &'static str)> {
        let media = match path.rsplit_once('.')?.1 {
            "svg" => "image/svg+xml",
            "png" => "image/png",
            _ => return None,
        };
        if !path.starts_with("characters/") {
            return None;
        }
        let installed = self.installed.read().unwrap_or_else(|e| e.into_inner());
        let p = installed.iter().find(|p| p.id() == pack && p.check.ok())?;
        Some((p.files.get(path)?.clone(), media))
    }
}

/// Knock's own pack, as the list shows it.
fn classic() -> PackSummary {
    let names = [
        ("fox", "Fox"),
        ("rabbit", "Rabbit"),
        ("cat", "Cat"),
        ("owl", "Owl"),
    ];
    PackSummary {
        id: CLASSIC.into(),
        display_name: "Classic".into(),
        author: Some("Knock".into()),
        license: Some("MIT".into()),
        description: Some("Knock's own fox, rabbit, cat and owl.".into()),
        builtin: true,
        valid: true,
        errors: Vec::new(),
        characters: names
            .iter()
            .map(|(id, name)| CharacterInfo {
                id: (*id).into(),
                display_name: (*name).into(),
                kind: Kind::Rig,
            })
            .collect(),
    }
}

/// Every pack folder in `dir`, checked, by display name. Links and files in
/// `dir` are skipped: a pack is a real folder.
fn scan(dir: &Path) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Installed> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let folder = e.file_name().into_string().ok()?;
            // Half-installed or hidden folders (".install-…") aren't packs.
            if folder.starts_with('.') {
                return None;
            }
            Some(load(&e.path(), folder))
        })
        .collect();
    out.sort_by(|a, b| {
        let name = |p: &Installed| {
            (p.check.manifest.as_ref()).map_or_else(
                || p.folder.to_lowercase(),
                |m| m.display_name.to_lowercase(),
            )
        };
        name(a).cmp(&name(b))
    });
    out
}

fn load(path: &Path, folder: String) -> Installed {
    let (check, files) = match packs::read_folder(path) {
        Err(e) => (
            PackCheck {
                errors: vec![e],
                ..PackCheck::default()
            },
            PackFiles::new(),
        ),
        Ok(files) => {
            let mut check = packs::check_pack(&files);
            // The folder is named after the pack, so two folders can't
            // claim the same id.
            if let Some(Manifest { id, .. }) = &check.manifest
                && *id != folder
            {
                check
                    .errors
                    .push(format!("the pack's folder must be named \"{id}\""));
            }
            (check, files)
        }
    };
    let files = if check.ok() { files } else { PackFiles::new() };
    Installed {
        folder,
        check,
        files,
    }
}

/// Percent-decodes a `knockpack://` request path (`convertFileSrc` encodes
/// the whole path, slashes included) and splits it into pack and file.
/// `None` for anything that isn't `<pack>/<path>` in plain text.
pub fn request_path(uri_path: &str) -> Option<(String, String)> {
    let raw = uri_path.trim_start_matches('/');
    let mut bytes = Vec::with_capacity(raw.len());
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    let (pack, path) = decoded.split_once('/')?;
    // Paths are only ever looked up in memory, but keep them plain anyway.
    if !packs::is_id(pack)
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    Some((pack.to_string(), path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("knock-packs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A one-fox pack copied from Knock's own art, in `dir/<id>`.
    fn fox_pack(dir: &Path, id: &str, folder: &str) {
        let art = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../art");
        let to = dir.join(folder).join("characters").join("fox");
        std::fs::create_dir_all(&to).unwrap();
        for f in std::fs::read_dir(art.join("animals/fox")).unwrap() {
            let f = f.unwrap();
            std::fs::copy(f.path(), to.join(f.file_name())).unwrap();
        }
        let m =
            format!(r#"{{"format":1,"id":"{id}","displayName":"Pack {id}","characters":["fox"]}}"#);
        std::fs::write(dir.join(folder).join("pack.json"), m).unwrap();
    }

    #[test]
    fn packs_are_found_checked_and_served_from_memory() {
        let d = temp("found");
        fox_pack(&d, "good", "good");
        fox_pack(&d, "named", "wrong-folder");
        std::fs::create_dir_all(d.join("broken")).unwrap();
        std::fs::write(d.join("broken/pack.json"), b"{").unwrap();
        std::fs::create_dir_all(d.join(".install-123")).unwrap();
        let packs = Packs::new(Some(d.clone()));
        let list = packs.list();
        let ids: Vec<_> = list.iter().map(|p| (p.id.as_str(), p.valid)).collect();
        assert_eq!(
            ids,
            [
                ("classic", true),
                ("broken", false),
                ("good", true),
                ("named", false)
            ]
        );
        assert!(list[3].errors[0].contains("folder must be named \"named\""));
        assert_eq!(packs.cast("good"), Some(vec!["fox".to_string()]));
        assert_eq!(packs.cast("named"), None, "invalid packs aren't used");
        let data = packs.get("good").unwrap();
        assert_eq!(data.characters[0].rig.as_ref().unwrap()["id"], "fox");
        // Served from memory: deleting the file changes nothing until reload.
        std::fs::remove_file(d.join("good/characters/fox/head.svg")).unwrap();
        let (svg, media) = packs.image("good", "characters/fox/head.svg").unwrap();
        assert!(svg.starts_with(b"<svg") && media == "image/svg+xml");
        assert!(packs.image("good", "pack.json").is_none(), "only images");
        assert!(packs.image("good", "characters/fox/rig.json").is_none());
        assert!(packs.image("named", "characters/fox/head.svg").is_none());
        packs.reload();
        assert!(!packs.list().iter().any(|p| p.id == "good" && p.valid));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_folder_means_classic_only() {
        let packs = Packs::new(None);
        assert_eq!(packs.list().len(), 1);
        assert_eq!(packs.list()[0].id, CLASSIC);
        assert!(packs.get(CLASSIC).is_none());
    }

    #[test]
    fn request_paths_are_decoded_and_plain() {
        assert_eq!(
            request_path("/good%2Fcharacters%2Ffox%2Fhead.svg"),
            Some(("good".into(), "characters/fox/head.svg".into()))
        );
        assert_eq!(
            request_path("/good/characters/fox/head.svg"),
            Some(("good".into(), "characters/fox/head.svg".into()))
        );
        for bad in [
            "/good%2F..%2F..%2Fetc%2Fpasswd",
            "/..%2Fx",
            "/Good/characters/x.svg",
            "/good",
            "/good/characters//x.svg",
            "/good/%ZZ",
            "/good/%",
        ] {
            assert_eq!(request_path(bad), None, "{bad}");
        }
    }
}
