//! Knock helper CLI (SPEC.md §7.1, §13.3).

mod fixtures;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["fixtures", "sanitize", rec, out, "--map", map] => {
            match fixtures::run(rec.as_ref(), out.as_ref(), map.as_ref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            println!(
                "usage: knock <sim <scenario> | replay <fixture> | doctor \
                 | fixtures sanitize <recording.jsonl> <out-dir> --map <names-file>>"
            );
            ExitCode::SUCCESS
        }
    }
}
