//! `knock` with bad or missing arguments prints usage to stderr and fails.

use std::process::Command;

#[test]
fn bad_or_missing_arguments_print_usage_and_fail() {
    for args in [
        &[][..],
        &["nope"],
        &["fixtures", "sanitize", "x"],
        &["fixtures", "check"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_knock"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: knock"));
    }
}
