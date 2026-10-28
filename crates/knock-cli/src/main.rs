//! Knock helper CLI (SPEC.md §7.1, §13.3).

mod fixtures;
mod replay;
mod sim;

use std::process::ExitCode;

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
                 | fixtures check <dir>>"
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
