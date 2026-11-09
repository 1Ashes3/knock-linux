//! Sprite pack format and checks (ADR 0038).

use knock_core::packs::{Kind, PackFiles, check_pack, check_svg, png_size};
use std::path::Path;

fn art() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../art"))
}

/// Knock's own animals and clips, laid out as a pack.
fn builtin() -> PackFiles {
    let mut files = PackFiles::new();
    let mut ids = Vec::new();
    for animal in std::fs::read_dir(art().join("animals")).unwrap() {
        let animal = animal.unwrap();
        let id = animal.file_name().into_string().unwrap();
        for f in std::fs::read_dir(animal.path()).unwrap() {
            let f = f.unwrap();
            let name = f.file_name().into_string().unwrap();
            files.insert(
                format!("characters/{id}/{name}"),
                std::fs::read(f.path()).unwrap(),
            );
        }
        ids.push(id);
    }
    ids.sort();
    for clip in std::fs::read_dir(art().join("clips")).unwrap() {
        let clip = clip.unwrap();
        let name = clip.file_name().into_string().unwrap();
        files.insert(format!("clips/{name}"), std::fs::read(clip.path()).unwrap());
    }
    let manifest = serde_json::json!({
        "format": 1, "id": "builtin", "displayName": "Built-in", "characters": ids
    });
    files.insert("pack.json".into(), manifest.to_string().into_bytes());
    files
}

fn errors(files: &PackFiles) -> Vec<String> {
    check_pack(files).errors
}

/// Some error mentions `needle`.
fn rejects(files: &PackFiles, needle: &str) {
    let e = errors(files);
    assert!(
        e.iter().any(|x| x.contains(needle)),
        "expected {needle:?} in {e:#?}"
    );
}

#[test]
fn knocks_own_art_is_a_valid_pack() {
    let check = check_pack(&builtin());
    assert_eq!(check.errors, Vec::<String>::new());
    assert!(check.ok());
    let ids: Vec<_> = check.characters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["cat", "fox", "owl", "rabbit"]);
    assert!(check.characters.iter().all(|c| c.kind == Kind::Rig));
}

#[test]
fn only_known_files_in_known_places() {
    for path in [
        "../escape.json",
        "characters/fox/../../escape.svg",
        "characters/Fox/head.svg",
        "characters/fox/sub/head.svg",
        "characters/fox/.hidden.svg",
        "characters/fox/run.sh",
        "characters/fox/head.svg.exe",
        "/etc/passwd",
        "clips/sub/knock.json",
        "notes.txt",
        "pack.json/x",
    ] {
        let mut f = builtin();
        f.insert(path.into(), b"{}".to_vec());
        rejects(&f, &format!("{path}: not a file a pack may contain"));
    }
    let mut f = builtin();
    f.insert("README.md".into(), b"# Hi".to_vec());
    f.insert("LICENSE".into(), b"MIT".to_vec());
    assert!(check_pack(&f).ok(), "docs are allowed");
}

#[test]
fn size_limits() {
    let mut f = builtin();
    f.insert("characters/fox/huge.svg".into(), vec![b' '; 300 * 1024]);
    rejects(&f, "larger than 256 KB");
    let mut f = builtin();
    for i in 0..400 {
        f.insert(format!("characters/fox/x{i}.svg"), Vec::new());
    }
    rejects(&f, "at most 300 allowed");
    let mut f = builtin();
    for i in 0..5 {
        f.insert(format!("characters/fox/p{i}.png"), vec![0; 4 * 1024 * 1024]);
    }
    rejects(&f, "MB, at most 16 MB allowed");
}

fn with_head(svg: &str) -> PackFiles {
    let mut f = builtin();
    f.insert("characters/fox/head.svg".into(), svg.as_bytes().to_vec());
    f
}

const OK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><path d="M0 0" fill="#e0702a"/></svg>"##;

#[test]
fn part_svgs_are_flat_shapes_with_nothing_that_runs_or_loads() {
    assert!(check_pack(&with_head(OK_SVG)).ok());
    for (bad, why) in [
        (
            r#"<svg viewBox="0 0 64 64"><script>alert(1)</script></svg>"#,
            "<script",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><path onload="x()"/></svg>"#,
            "event handler",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><path ONCLICK = "x()"/></svg>"#,
            "event handler",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><image href="http://x/y.png"/></svg>"#,
            "<image",
        ),
        (
            r##"<svg viewBox="0 0 64 64"><use href="#a"/></svg>"##,
            "<use",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><path xlink:href="x"/></svg>"#,
            "href",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><path fill="url(http://x)"/></svg>"#,
            "url(",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><style>@import 'x';</style></svg>"#,
            "<style",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><path style="fill:url(#g)"/></svg>"#,
            "url(",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><a href="javascript:x"><path/></a></svg>"#,
            "<a",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><foreignObject/></svg>"#,
            "<foreignobject",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><set attributeName="x"/></svg>"#,
            "<set",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><linearGradient/></svg>"#,
            "<lineargradient",
        ),
        (
            r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa">]><svg viewBox="0 0 64 64">&a;</svg>"#,
            "not an SVG",
        ),
        (
            r#"<svg viewBox="0 0 64 64"><!ENTITY a "aaaa"><path/></svg>"#,
            "<!entity",
        ),
        (r#"<svg viewBox="0 0 64 64"><![CDATA[x]]></svg>"#, "cdata"),
        (r#"<svg viewBox="0 0 32 32"><path/></svg>"#, "viewBox"),
        (
            r#"<html><svg viewBox="0 0 64 64"></svg></html>"#,
            "not an SVG",
        ),
        ("not svg at all", "not an SVG"),
    ] {
        let e = errors(&with_head(bad));
        assert!(
            e.iter()
                .any(|x| x.starts_with("characters/fox/head.svg") && x.contains(why)),
            "{bad} → {e:#?}"
        );
    }
}

#[test]
fn colors_are_counted_like_art_check() {
    let svg = r##"<svg viewBox="0 0 64 64"><path fill="#abc" stroke="#AABBCC" stroke-width="2" fill-opacity="1"/><path style="fill: red; stroke:none"/></svg>"##;
    let mut c = check_svg(svg).unwrap();
    c.sort();
    c.dedup();
    assert_eq!(c, ["#aabbcc", "red"]);
    let many = (0..7)
        .map(|i| format!(r##"<path fill="#00000{i}"/>"##))
        .collect::<String>();
    let svg = format!(r#"<svg viewBox="0 0 64 64">{many}</svg>"#);
    rejects(&with_head(&svg), "at most 6 allowed");
}

#[test]
fn the_manifest_is_strict() {
    let set = |json: serde_json::Value| {
        let mut f = builtin();
        f.insert("pack.json".into(), json.to_string().into_bytes());
        f
    };
    let chars = ["cat", "fox", "owl", "rabbit"];
    rejects(
        &set(serde_json::json!({"format": 2, "id": "p", "displayName": "P", "characters": chars})),
        "format 2",
    );
    rejects(
        &set(serde_json::json!({"format": 1, "id": "P!", "displayName": "P", "characters": chars})),
        "id must be",
    );
    rejects(
        &set(
            serde_json::json!({"format": 1, "id": "classic", "displayName": "P", "characters": chars}),
        ),
        "Knock's own pack",
    );
    rejects(
        &set(
            serde_json::json!({"format": 1, "id": "p", "displayName": "P", "characters": chars, "run": "x"}),
        ),
        "unknown field",
    );
    rejects(
        &set(serde_json::json!({"format": 1, "id": "p", "displayName": "", "characters": chars})),
        "displayName",
    );
    rejects(
        &set(
            serde_json::json!({"format": 1, "id": "p", "displayName": "P", "characters": ["fox", "fox"]}),
        ),
        "listed twice",
    );
    rejects(
        &set(
            serde_json::json!({"format": 1, "id": "p", "displayName": "P", "characters": ["fox"]}),
        ),
        "characters/cat: not listed",
    );
    rejects(
        &set(
            serde_json::json!({"format": 1, "id": "p", "displayName": "P", "characters": ["fox", "ghost"]}),
        ),
        "characters/ghost: needs rig.json or sheet.json",
    );
    let mut f = builtin();
    f.remove("pack.json");
    rejects(&f, "pack.json is missing");
}

#[test]
fn rigs_need_every_part_and_their_files() {
    let mut f = builtin();
    f.remove("characters/fox/tail.svg");
    rejects(&f, "file tail.svg is missing");
    let mut f = builtin();
    let rig = r#"{"id":"fox","displayName":"Fox","viewBox":[0,0,64,64],"parts":[{"name":"body","file":"../cat/body.svg","pivot":[0,0],"z":0}]}"#;
    f.insert("characters/fox/rig.json".into(), rig.as_bytes().to_vec());
    rejects(&f, "file must be an .svg file in the character's folder");
    rejects(&f, "missing required part head");
}

/// A PNG header of `w`×`h`; enough for [`png_size`].
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    b.extend(b"IHDR");
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

fn sheet_pack(sheet: serde_json::Value, image: Vec<u8>) -> PackFiles {
    let mut f = PackFiles::new();
    let m = serde_json::json!({"format": 1, "id": "pixel", "displayName": "Pixel", "characters": ["frog"]});
    f.insert("pack.json".into(), m.to_string().into_bytes());
    f.insert(
        "characters/frog/sheet.json".into(),
        sheet.to_string().into_bytes(),
    );
    f.insert("characters/frog/frog.png".into(), image);
    f
}

fn sheet(clips: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": "frog", "displayName": "Frog", "image": "frog.png",
        "frameWidth": 32, "frameHeight": 32, "pixelArt": true, "clips": clips
    })
}

fn five_clips() -> serde_json::Value {
    serde_json::json!({
        "sleep": {"row": 0, "frames": 4, "fps": 4, "loop": true},
        "work":  {"row": 1, "frames": 4, "fps": 8, "loop": true},
        "wait":  {"row": 2, "frames": 2, "fps": 2, "loop": true},
        "knock": {"row": 3, "frames": 6, "fps": 12, "loop": true},
        "rest":  {"row": 4, "frames": 1, "fps": 1, "loop": false}
    })
}

#[test]
fn sprite_sheets() {
    assert_eq!(png_size(&png(192, 160)), Some((192, 160)));
    assert_eq!(png_size(b"GIF89a................"), None);
    let ok = check_pack(&sheet_pack(sheet(five_clips()), png(192, 160)));
    assert_eq!(ok.errors, Vec::<String>::new());
    assert_eq!(ok.characters[0].kind, Kind::Sheet);
    // Frames past the image's edge, by width and by row.
    rejects(
        &sheet_pack(sheet(five_clips()), png(160, 160)),
        "clip knock: its frames run past the edge",
    );
    rejects(
        &sheet_pack(sheet(five_clips()), png(192, 128)),
        "clip rest: its frames run past the edge",
    );
    let mut c = five_clips();
    c.as_object_mut().unwrap().remove("knock");
    rejects(
        &sheet_pack(sheet(c), png(192, 160)),
        "missing required clip knock",
    );
    let mut c = five_clips();
    c["dance"] = serde_json::json!({"row": 0, "frames": 1, "fps": 1, "loop": true});
    rejects(
        &sheet_pack(sheet(c), png(192, 160)),
        "clip dance: not a clip Knock plays",
    );
    let mut c = five_clips();
    c["sleep"]["fps"] = serde_json::json!(0);
    rejects(&sheet_pack(sheet(c), png(192, 160)), "fps must be above 0");
    rejects(
        &sheet_pack(sheet(five_clips()), b"not a png at all, nope".to_vec()),
        "is not a PNG image",
    );
    rejects(
        &sheet_pack(sheet(five_clips()), png(9000, 160)),
        "must be at most 4096",
    );
    let mut s = sheet(five_clips());
    s["image"] = serde_json::json!("../fox/x.png");
    rejects(
        &sheet_pack(s, png(192, 160)),
        "image must be a .png file in the character's folder",
    );
    let mut s = sheet(five_clips());
    s["shader"] = serde_json::json!("x");
    rejects(&sheet_pack(s, png(192, 160)), "unknown field");
}

#[test]
fn pack_clips_replace_shared_ones_for_rigs_only() {
    let mut f = builtin();
    let knock = r#"{"name":"knock","loop":true,"durationMs":800,"tracks":{"paw_r":[{"t":0,"y":-10},{"t":400,"y":-14}]},"swaps":[]}"#;
    f.insert("clips/knock.json".into(), knock.as_bytes().to_vec());
    assert!(check_pack(&f).ok());
    let wing = r#"{"name":"knock","loop":true,"durationMs":800,"tracks":{"wing":[{"t":0,"rot":5}]},"swaps":[]}"#;
    f.insert("clips/knock.json".into(), wing.as_bytes().to_vec());
    rejects(&f, "track wing: unknown part");
    f.insert("clips/dance.json".into(), knock.as_bytes().to_vec());
    rejects(&f, "clips/dance.json: not a clip Knock plays");
    let mut s = sheet_pack(sheet(five_clips()), png(192, 160));
    s.insert("clips/knock.json".into(), knock.as_bytes().to_vec());
    rejects(&s, "this pack has no rigs");
}

#[cfg(feature = "fs")]
mod folder {
    use super::*;
    use knock_core::packs::read_folder;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("knock-pack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, files: &PackFiles) {
        for (rel, bytes) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
    }

    #[test]
    fn a_folder_reads_back_as_the_same_files() {
        let d = temp("roundtrip");
        let files = builtin();
        write(&d, &files);
        assert_eq!(read_folder(&d).unwrap(), files);
        assert!(check_pack(&read_folder(&d).unwrap()).ok());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_and_special_files_are_refused() {
        let d = temp("links");
        write(&d, &builtin());
        // A link out of the pack, at a name a pack may use.
        std::os::unix::fs::symlink("/etc/hostname", d.join("README.md")).unwrap();
        let e = read_folder(&d).unwrap_err();
        assert!(e.contains("README.md: links are not allowed"), "{e}");
        std::fs::remove_file(d.join("README.md")).unwrap();
        // A linked folder.
        std::os::unix::fs::symlink("/etc", d.join("characters").join("etc")).unwrap();
        let e = read_folder(&d).unwrap_err();
        assert!(e.contains("characters/etc: links are not allowed"), "{e}");
        std::fs::remove_file(d.join("characters").join("etc")).unwrap();
        // The pack folder itself as a link.
        let link = temp("links-to").join("pack");
        std::os::unix::fs::symlink(&d, &link).unwrap();
        assert_eq!(read_folder(&link).unwrap_err(), "the pack is not a folder");
        std::fs::remove_dir_all(link.parent().unwrap()).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn deep_or_huge_folders_stop_early() {
        let d = temp("deep");
        std::fs::create_dir_all(d.join("a/b/c")).unwrap();
        std::fs::write(d.join("a/b/c/x.svg"), b"").unwrap();
        assert!(read_folder(&d).unwrap_err().contains("nest too deep"));
        std::fs::remove_dir_all(&d).unwrap();
        let d = temp("many");
        for i in 0..301 {
            std::fs::write(d.join(format!("f{i}")), b"").unwrap();
        }
        assert!(read_folder(&d).unwrap_err().contains("more than 300 files"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
