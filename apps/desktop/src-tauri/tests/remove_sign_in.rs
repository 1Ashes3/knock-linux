//! The uninstaller's `knock-desktop.exe --remove-sign-in` (installer-hooks.nsh,
//! ADR 0025), end to end on the real per-user Run key, so only on request:
//! `KNOCK_AUTOSTART_TEST=1 cargo test -p knock-desktop --test remove_sign_in -- --ignored`.
//! Removes what it adds.

use std::process::Command;
use std::time::{Duration, Instant};

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// `reg.exe` with `args`: its output if it succeeded.
fn reg(args: &[&str]) -> Option<String> {
    let out = Command::new("reg.exe").args(args).output().unwrap();
    (out.status.success()).then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn knock(key: &str) -> Option<String> {
    reg(&["query", key, "/v", "Knock"])
}

fn set(kind: &str, data: &str) {
    reg(&["add", RUN, "/v", "Knock", "/t", kind, "/d", data, "/f"]).unwrap();
    // Disabled in Task Manager: 03 00 00 00, then a FILETIME.
    let mark = "030000000000000000000000";
    reg(&[
        "add",
        APPROVED,
        "/v",
        "Knock",
        "/t",
        "REG_BINARY",
        "/d",
        mark,
        "/f",
    ])
    .unwrap();
}

/// Runs the app with the flag. It must exit by itself, at once and with 0:
/// the uninstaller waits for it.
fn remove_sign_in() {
    let mut app = Command::new(env!("CARGO_BIN_EXE_knock-desktop"))
        .arg("--remove-sign-in")
        .spawn()
        .unwrap();
    let t = Instant::now();
    let status = loop {
        if let Some(s) = app.try_wait().unwrap() {
            break s;
        }
        if t.elapsed() > Duration::from_secs(10) {
            let _ = app.kill();
            panic!("still running: the uninstaller would wait forever");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
}

#[test]
#[ignore = "writes the real HKCU Run key; set KNOCK_AUTOSTART_TEST=1"]
fn the_flag_removes_only_an_entry_that_starts_knock_and_exits() {
    assert_eq!(std::env::var("KNOCK_AUTOSTART_TEST").as_deref(), Ok("1"));
    assert_eq!(knock(RUN), None, "an entry was already there");
    // Removes both values even if an assertion below fails.
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for key in [RUN, APPROVED] {
                let _ = reg(&["delete", key, "/v", "Knock", "/f"]);
            }
        }
    }
    let _cleanup = Cleanup;

    // Knock's app, in another folder, expandable: the value and its mark go.
    set(
        "REG_EXPAND_SZ",
        r#""%LOCALAPPDATA%\Old\knock-desktop.exe" --x"#,
    );
    remove_sign_in();
    assert_eq!((knock(RUN), knock(APPROVED)), (None, None));
    // Nothing there: nothing to do, still a clean exit.
    remove_sign_in();
    // The CLI (knock.exe) is another program: both stay.
    let cli = r#""C:\Tools\knock.exe" sim demo"#;
    set("REG_SZ", cli);
    remove_sign_in();
    let kept = (knock(RUN), knock(APPROVED));
    assert!(kept.0.is_some_and(|v| v.contains(cli)));
    assert!(kept.1.is_some());
}
