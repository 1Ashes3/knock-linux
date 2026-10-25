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

#[test]
fn check_of_a_folder_with_no_fixtures_fails() {
    let dir = std::env::temp_dir().join(format!("knock-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_knock"))
        .args(["fixtures", "check"])
        .arg(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "error: no fixture files found, 0 checked"
    );
}
