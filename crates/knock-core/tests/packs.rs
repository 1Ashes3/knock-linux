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
fn part_svgs_are_plain_shapes_on_an_allowlist() {
    assert!(check_pack(&with_head(OK_SVG)).ok());
    let ok = |inner: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" aria-hidden="true">{inner}</svg>"#
        )
    };
    assert!(check_svg(&ok(r##"<g transform="rotate(10 32 32)"><circle cx="1" cy="2" r="3" fill="#abc" stroke="none"/></g>"##)).is_ok());
    assert!(check_svg(&format!("<?xml version=\"1.0\"?>\n{}", ok(""))).is_ok());
    for (bad, why) in [
        // Elements and attributes outside the allowlist.
        (ok("<script>alert(1)</script>"), "uses <script>"),
        (ok(r#"<path onload="x()"/>"#), r#"attribute "onload""#),
        (ok(r#"<image href="http://x/y.png"/>"#), "uses <image>"),
        (ok(r##"<use href="#a"/>"##), "uses <use>"),
        (ok(r#"<path style="fill:red"/>"#), r#"attribute "style""#),
        (ok(r#"<path id="a" class="b"/>"#), r#"attribute "id""#),
        (ok("<style>@import 'x';</style>"), "uses <style>"),
        (ok("<foreignObject/>"), "uses <foreignObject>"),
        (ok(r#"<set attributeName="x"/>"#), "uses <set>"),
        (ok("<linearGradient/>"), "uses <linearGradient>"),
        (ok("<a\r><path/></a>"), "uses <a"),
        (ok(r#"<path filter="url(#f)"/>"#), r#"attribute "filter""#),
        // Security review finding 1: namespace prefixes, character references, src.
        (r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:s="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><s:script>x</s:script></svg>"#.into(), r#"attribute "xmlns:s""#),
        (ok("<s:script>x</s:script>"), "uses <s:script>"),
        (ok(r#"<path fill="url&#40;https://example.invalid/t.svg#a)"/>"#), "contains &"),
        (ok(r#"<h:img src="https://example.invalid/p.png"/>"#), "uses <h:img>"),
        (ok(r#"<path src="https://example.invalid/p.png"/>"#), r#"attribute "src""#),
        (ok(r#"<path xlink:href="x"/>"#), r#"attribute "xlink:href""#),
        (r#"<svg xmlns="http://evil.example/" viewBox="0 0 64 64"></svg>"#.into(), "xmlns must be"),
        // XML tricks.
        (r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa">]><svg viewBox="0 0 64 64">&a;</svg>"#.into(), "contains &"),
        (ok(r#"<!ENTITY a "aaaa"><path/>"#), "<! or <?"),
        (ok("<![CDATA[x]]>"), "<! or <?"),
        (ok("<!-- hi -->"), "<! or <?"),
        (ok("<?php x ?>"), "<! or <?"),
        (ok("text"), "text inside the drawing"),
        (format!("{}x", ok("")), "text outside the shapes"),
        (format!("{}<svg/>", ok("")), "after </svg>"),
        (ok("<svg viewBox=\"0 0 64 64\"/>"), "<svg> inside the <svg>"),
        (ok("<g><path/>"), "don't match"),
        (ok(r#"<path d="M0 0" d="M1 1"/>"#), "sets d twice"),
        (ok(r#"<path d=M0/>"#), "needs a quoted value"),
        (ok(r#"<path transform="translate(0;0)"/>"#), "characters that aren't allowed"),
        (r#"<svg viewBox="0 0 32 32"><path/></svg>"#.into(), "viewBox"),
        (r#"<html><svg viewBox="0 0 64 64"></svg></html>"#.into(), "uses <html>"),
        ("not svg at all".into(), "text outside"),
        // Colors: hex or none only (finding 6).
        (ok(r#"<path fill="rgb( 255,0,0)"/>"#), "hex color"),
        (ok(r#"<path fill="currentColor"/>"#), "hex color"),
        (ok(r#"<path stroke="red"/>"#), "hex color"),
    ] {
        let e = errors(&with_head(&bad));
        assert!(
            e.iter().any(|x| x.starts_with("characters/fox/head.svg") && x.contains(why)),
            "{bad} → {e:#?}"
        );
    }
    let deep = format!("{}{}", "<g>".repeat(20), "</g>".repeat(20));
    assert!(check_svg(&ok(&deep)).unwrap_err().contains("too deeply"));
}

#[test]
fn colors_are_counted_per_character() {
    let svg = r##"<svg viewBox="0 0 64 64"><path fill="#abc" stroke="#AABBCC" stroke-width="2" fill-opacity="1"/><path fill="#E0702A" stroke="none"/></svg>"##;
    let mut c = check_svg(svg).unwrap();
    c.sort();
    c.dedup();
    assert_eq!(c, ["#aabbcc", "#e0702a"]);
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

/// Security review finding 3: a rig has exactly the 10 parts, nothing else.
#[test]
fn rigs_are_typed_with_exactly_the_ten_parts() {
    let fox: serde_json::Value =
        serde_json::from_slice(&builtin()["characters/fox/rig.json"]).unwrap();
    let set = |rig: serde_json::Value| {
        let mut f = builtin();
        f.insert(
            "characters/fox/rig.json".into(),
            rig.to_string().into_bytes(),
        );
        f
    };
    let mut extra = fox.clone();
    extra["extraTopLevelField"] = true.into();
    rejects(&set(extra), "unknown field");
    let mut odd = fox.clone();
    odd["parts"].as_array_mut().unwrap().push(serde_json::json!(
        {"name": "\"><script>bad</script>", "file": "body.svg", "pivot": [0, 0], "z": 7}
    ));
    rejects(&set(odd), "isn't one of body");
    let mut twice = fox.clone();
    let body = twice["parts"][2].clone();
    twice["parts"].as_array_mut().unwrap().push(body);
    rejects(&set(twice), "part body: listed twice");
    let mut field = fox.clone();
    field["parts"][0]["onload"] = "x".into();
    rejects(&set(field), "unknown field");
    let mut huge = fox;
    huge["parts"][0]["z"] = 1e300.into();
    rejects(&set(huge), "pivot and z must be plain numbers");
}

/// Security review finding 2: every file in a character's folder is used,
/// so none goes unchecked.
#[test]
fn files_a_character_doesnt_use_are_refused() {
    for (extra, bytes) in [
        (
            "unreferenced.svg",
            b"<svg><script>x</script></svg>".to_vec(),
        ),
        ("extra.json", b"{}".to_vec()),
        ("extra.png", png(8, 8)),
    ] {
        let mut f = builtin();
        f.insert(format!("characters/fox/{extra}"), bytes);
        rejects(
            &f,
            &format!("characters/fox/{extra}: not used by its character"),
        );
    }
    let mut s = sheet_pack(sheet(five_clips()), png(192, 160));
    s.insert("characters/frog/other.png".into(), png(8, 8));
    rejects(&s, "characters/frog/other.png: not used");
}

/// A PNG's header and end, enough for [`png_size`] (no pixel data).
fn png_with(w: u32, h: u32, depth: u8, color: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    b.extend(b"IHDR");
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([depth, color, 0, 0, 0, 0, 0, 0, 0]);
    b.extend([0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
    b
}

fn png(w: u32, h: u32) -> Vec<u8> {
    png_with(w, h, 8, 6)
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

/// Security review finding 4: well-formed PNGs only, and a pixel budget.
#[test]
fn png_headers_are_strict_and_pixels_are_budgeted() {
    assert_eq!(png_size(&png_with(64, 64, 4, 3)), Some((64, 64)), "palette");
    let mut lie = png(4096, 4096);
    lie[8..12].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    assert_eq!(png_size(&lie), None, "IHDR length must be 13");
    assert_eq!(png_size(&png_with(64, 64, 16, 6)), None, "16-bit");
    assert_eq!(png_size(&png_with(64, 64, 8, 2)), None, "RGB without alpha");
    let mut cut = png(64, 64);
    cut.truncate(cut.len() - 12);
    assert_eq!(png_size(&cut), None, "no IEND");
    // Sixteen 1024×1024 sheets fit; one more megapixel doesn't.
    let mut f = PackFiles::new();
    let ids: Vec<String> = (0..16).map(|i| format!("c{i}")).collect();
    let m = serde_json::json!({"format": 1, "id": "big", "displayName": "Big", "characters": ids});
    f.insert("pack.json".into(), m.to_string().into_bytes());
    for (i, id) in ids.iter().enumerate() {
        let mut sh = sheet(five_clips());
        sh["id"] = id.clone().into();
        sh["image"] = "s.png".into();
        f.insert(
            format!("characters/{id}/sheet.json"),
            sh.to_string().into_bytes(),
        );
        let side = if i == 0 { 1100 } else { 1024 };
        f.insert(format!("characters/{id}/s.png"), png(side, 1024));
    }
    rejects(&f, "megapixels, at most 16 allowed");
}

/// Security review finding 6: clips have limits.
#[test]
fn clips_have_limits() {
    let set = |clip: serde_json::Value| {
        let mut f = builtin();
        f.insert("clips/knock.json".into(), clip.to_string().into_bytes());
        f
    };
    let base = || serde_json::json!({"name": "knock", "loop": true, "durationMs": 1000, "tracks": {}, "swaps": []});
    let mut long = base();
    long["durationMs"] = 1e300.into();
    rejects(&set(long), "at most 60000");
    let mut many = base();
    let keys: Vec<_> = (0..300).map(|t| serde_json::json!({"t": t})).collect();
    many["tracks"]["head"] = keys.into();
    rejects(&set(many), "at most 256 keyframes");
    for (field, value) in [
        ("x", 1e9),
        ("rot", 99999.0),
        ("scaleX", -1.0),
        ("alpha", 2.0),
    ] {
        let mut c = base();
        c["tracks"]["head"] = serde_json::json!([{ "t": 0, field: value }]);
        rejects(&set(c), &format!("{field} must be from"));
    }
    let mut swaps = base();
    let s: Vec<_> = (0..65)
        .map(
            |_| serde_json::json!({"from": "eyes_open", "to": "eyes_closed", "start": 0, "end": 1}),
        )
        .collect();
    swaps["swaps"] = s.into();
    rejects(&set(swaps), "at most 64 swaps");
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
    fn unexpected_folders_names_and_huge_folders_stop_early() {
        let d = temp("deep");
        std::fs::create_dir_all(d.join("a/b/c")).unwrap();
        assert!(
            read_folder(&d)
                .unwrap_err()
                .contains("a: not a folder a pack may contain")
        );
        std::fs::remove_dir_all(&d).unwrap();
        let d = temp("nested");
        std::fs::create_dir_all(d.join("characters/fox/more")).unwrap();
        let e = read_folder(&d).unwrap_err();
        assert!(e.contains("characters/fox/more: not a folder"), "{e}");
        std::fs::remove_dir_all(&d).unwrap();
        // A badly named file is refused before it is read.
        let d = temp("name");
        std::fs::write(d.join("movie.mkv"), b"").unwrap();
        assert_eq!(
            read_folder(&d).unwrap_err(),
            "movie.mkv: not a file a pack may contain"
        );
        std::fs::remove_dir_all(&d).unwrap();
        let d = temp("many");
        std::fs::create_dir_all(d.join("characters/fox")).unwrap();
        for i in 0..301 {
            std::fs::write(d.join(format!("characters/fox/f{i}.svg")), b"").unwrap();
        }
        assert!(read_folder(&d).unwrap_err().contains("more than 300 files"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Security review finding 5: a named pipe is refused, never waited on.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_without_blocking() {
        let d = temp("fifo");
        let fifo = d.join("README.md");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            read_folder(&d).unwrap_err(),
            "README.md: not a regular file"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[cfg(feature = "zip")]
mod zipped {
    use super::*;
    use knock_core::packs::{read_zip, write_zip};
    use std::io::Write;

    /// A zip with exactly these entries, names as given.
    fn raw_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            z.start_file(*name, o).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn a_built_pack_reads_back_and_passes() {
        let files = builtin();
        let bytes = write_zip(&files).unwrap();
        assert_eq!(read_zip(&bytes).unwrap(), files);
        assert!(check_pack(&read_zip(&bytes).unwrap()).ok());
    }

    #[test]
    fn one_top_folder_is_dropped() {
        let bytes = raw_zip(&[("fairies/pack.json", b"{}"), ("fairies/README.md", b"hi")]);
        let files = read_zip(&bytes).unwrap();
        assert_eq!(files.keys().collect::<Vec<_>>(), ["README.md", "pack.json"]);
        // Two top folders: kept as they are (and check_pack refuses them).
        let bytes = raw_zip(&[("a/pack.json", b"{}"), ("b/x.svg", b"")]);
        assert!(read_zip(&bytes).unwrap().contains_key("a/pack.json"));
    }

    #[test]
    fn names_that_could_escape_are_refused() {
        for name in [
            "../evil.json",
            "characters/../../evil.svg",
            "/etc/passwd",
            "characters\\fox\\head.svg",
            "C:/Windows/evil.svg",
            "characters//fox.svg",
            "./pack.json",
        ] {
            let e = read_zip(&raw_zip(&[(name, b"x")])).unwrap_err();
            assert!(e.contains("not a safe file name"), "{name}: {e}");
        }
    }

    #[test]
    fn links_are_refused() {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        z.add_symlink("README.md", "/home/someone/.ssh/id_ed25519", o)
            .unwrap();
        let bytes = z.finish().unwrap().into_inner();
        assert!(
            read_zip(&bytes)
                .unwrap_err()
                .contains("links are not allowed")
        );
    }

    #[test]
    fn a_zip_bomb_stops_at_the_size_limit() {
        // 20 MB of zeros deflates to a few KB; the headers aren't trusted.
        let zeros = vec![0u8; 20 * 1024 * 1024];
        let bytes = raw_zip(&[("characters/fox/head.svg", &zeros)]);
        assert!(bytes.len() < 100 * 1024, "{}", bytes.len());
        let e = read_zip(&bytes).unwrap_err();
        assert!(e.contains("unpacks to more than 16 MB"), "{e}");
    }

    #[test]
    fn too_many_entries_and_non_zips() {
        let names: Vec<String> = (0..301)
            .map(|i| format!("characters/fox/f{i}.svg"))
            .collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        assert!(
            read_zip(&raw_zip(&entries))
                .unwrap_err()
                .contains("more than 300 files")
        );
        assert!(
            read_zip(b"not a zip at all")
                .unwrap_err()
                .contains("isn't a .knockpack")
        );
    }
}
