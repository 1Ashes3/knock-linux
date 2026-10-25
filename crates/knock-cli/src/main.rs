//! Knock helper CLI (SPEC.md §7.1, §13.3).

mod fixtures;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
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
        _ => {
            eprintln!(
                "usage: knock <sim <scenario> | replay <fixture> | doctor \
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
