//! Knock helper CLI (SPEC.md §7.1, §13.3).

mod fixtures;
mod replay;
mod sim;

use std::process::ExitCode;

/// `knock pack check <folder>`: the same checks Knock runs before it offers
/// a pack (ADR 0038), for artists.
fn pack_check(dir: &std::path::Path) -> Result<(), String> {
    let files = knock_core::packs::read_folder(dir)?;
    let check = knock_core::packs::check_pack(&files);
    if check.ok() {
        let name = check
            .manifest
            .as_ref()
            .map_or("", |m| m.display_name.as_str());
        let n = check.characters.len();
        let s = if n == 1 { "" } else { "s" };
        println!("{name}: valid, {n} character{s}");
        for c in &check.characters {
            let kind = match c.kind {
                knock_core::packs::Kind::Rig => "vector rig",
                knock_core::packs::Kind::Sheet => "sprite sheet",
            };
            println!("  {} ({}): {kind}", c.display_name, c.id);
        }
        return Ok(());
    }
    for e in &check.errors {
        println!("  {e}");
    }
    let n = check.errors.len();
    let s = if n == 1 { "" } else { "s" };
    Err(format!("{n} problem{s}; the pack can't be used yet"))
}

/// `knock pack build <folder> [-o <file>]`: checks the pack, then writes it
/// as one `.knockpack` file (default `<id>.knockpack` here) that users add
/// with Settings → Characters → Add a pack, or by double-clicking it.
fn pack_build(dir: &std::path::Path, out: Option<&std::path::Path>) -> Result<(), String> {
    pack_check(dir)?;
    let files = knock_core::packs::read_folder(dir)?;
    let id = knock_core::packs::check_pack(&files)
        .manifest
        .map(|m| m.id)
        .ok_or("the pack has no id")?;
    let bytes = knock_core::packs::write_zip(&files)?;
    let out = out.map_or_else(
        || std::path::PathBuf::from(format!("{id}.knockpack")),
        Into::into,
    );
    std::fs::write(&out, &bytes)
        .map_err(|e| format!("can't write {} ({})", out.display(), e.kind()))?;
    println!(
        "wrote {} ({} KB)",
        out.display(),
        bytes.len().div_ceil(1024)
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["sim", name] => sim::live(name, 1.0),
        ["sim", name, "--speed", f] => match f.parse() {
            Ok(f) => sim::live(name, f),
            Err(_) => Err("--speed must be a number above 0, like 2 or 0.5".into()),
        },
        ["sim", name, "--local"] => sim::local(name).map(|t| println!("{t}")),
        ["fixtures", "sanitize", rec, out, "--map", map] => {
            fixtures::run(rec.as_ref(), out.as_ref(), map.as_ref())
        }
        ["fixtures", "check", dir] => fixtures::check(dir.as_ref()).and_then(|n| match n {
            0 => Err("no fixture files found, 0 checked".into()),
            n => {
                println!("{n} files checked, all sanitized");
                Ok(())
            }
        }),
        ["pack", "check", dir] => pack_check(dir.as_ref()),
        ["pack", "build", dir] => pack_build(dir.as_ref(), None),
        ["pack", "build", dir, "-o", out] => pack_build(dir.as_ref(), Some(out.as_ref())),
        ["replay", target] => {
            // A fixture file, a folder of fixtures, or a name in `fixtures/`.
            let path = std::path::Path::new(target);
            let named = std::path::Path::new("fixtures").join(format!("{target}.jsonl"));
            let lines = if path.is_dir() {
                replay::all(path)
            } else if path.exists() {
                replay::one(path).map(|l| vec![l])
            } else {
                replay::one(&named).map(|l| vec![l])
            };
            lines.map(|l| {
                println!(
                    "{}",
                    l.join(
                        "
"
                    )
                )
            })
        }
        _ => {
            eprintln!(
                "usage: knock <sim <scenario> [--speed <factor> | --local] | replay <fixture | dir> | doctor \
                 | fixtures sanitize <recording.jsonl> <out-dir> --map <names-file> \
                 | fixtures check <dir> | pack check <folder> \
                 | pack build <folder> [-o <file.knockpack>]>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
