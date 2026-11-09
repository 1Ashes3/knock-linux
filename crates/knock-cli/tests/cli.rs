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

mod sim {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::mpsc;

    /// Made up, the shape of a real one (43 base64url characters).
    const TOKEN: &str = "test-token-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    /// A fake `%APPDATA%` whose `Knock\server.json` points at `port`.
    fn appdata(name: &str, port: Option<u16>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("knock-sim-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = knock_core::paths::data_dir(&|k: &str| super::test_env(&dir, k)).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        if let Some(port) = port {
            let cfg = format!(r#"{{"port":{port},"token":"{TOKEN}"}}"#);
            std::fs::write(data.join("server.json"), cfg).unwrap();
        }
        dir
    }

    fn knock(dir: &PathBuf, args: &[&str]) -> Output {
        let out = Command::new(env!("CARGO_BIN_EXE_knock"))
            .args(args)
            .env("APPDATA", dir)
            .env("XDG_CONFIG_HOME", dir)
            .output()
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        for s in [&out.stdout, &out.stderr] {
            assert!(!String::from_utf8_lossy(s).contains(TOKEN), "token printed");
        }
        out
    }

    /// Accepts `n` requests, answers each with `status`, sends back
    /// (head, body) per request.
    fn fake_knock(n: usize, status: &'static str) -> (u16, mpsc::Receiver<(String, String)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for conn in listener.incoming().take(n) {
                let mut conn = BufReader::new(conn.unwrap());
                let mut head = String::new();
                while !head.ends_with("\r\n\r\n") {
                    conn.read_line(&mut head).unwrap();
                }
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut body = vec![0; len];
                conn.read_exact(&mut body).unwrap();
                let reply = format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\n\r\n{{}}");
                conn.get_mut().write_all(reply.as_bytes()).unwrap();
                tx.send((head, String::from_utf8(body).unwrap())).unwrap();
            }
        });
        (port, rx)
    }

    #[test]
    fn posts_each_event_with_the_token_and_sim_header_and_skips_controls() {
        let (port, rx) = fake_knock(4, "200 OK");
        let dir = appdata("ok", Some(port));
        let out = knock(&dir, &["sim", "crash", "--speed", "1000"]);
        assert_eq!(out.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("s41  skipped process_gone"), "{stdout}");
        let scenario = include_str!("../scenarios/crash.jsonl");
        let payloads: Vec<String> = scenario
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|v| v.get("control").is_none())
            .map(|v| v["payload"].to_string())
            .collect();
        assert_eq!(payloads.len(), 4);
        for want in payloads {
            let (head, body) = rx.recv().unwrap();
            assert!(head.starts_with("POST /v1/hooks/claude-code HTTP/1.1\r\n"));
            assert!(head.contains(&format!("\r\nX-Knock-Token: {TOKEN}\r\n")));
            assert!(head.contains("\r\nX-Knock-Sim: 1\r\n"));
            assert_eq!(body, want);
        }
    }

    #[test]
    fn says_plainly_when_knock_is_not_running_or_refuses() {
        let no_file = knock(&appdata("none", None), &["sim", "demo"]);
        let closed = {
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            knock(&appdata("closed", Some(port)), &["sim", "demo"])
        };
        let (port, _rx) = fake_knock(1, "401 Unauthorized");
        let refused = knock(&appdata("401", Some(port)), &["sim", "demo"]);
        for (out, want) in [
            (no_file, "error: Knock doesn't seem to be running"),
            (
                closed,
                "error: Knock isn't running: nothing answered on 127.0.0.1:",
            ),
            (refused, "error: Knock refused the request"),
        ] {
            assert_eq!(out.status.code(), Some(1));
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(err.starts_with(want), "{err}");
        }
        let bad = knock(&appdata("speed", None), &["sim", "demo", "--speed", "0"]);
        assert!(String::from_utf8_lossy(&bad.stderr).contains("--speed must be a number above 0"));
        let unknown = knock(&appdata("unknown", None), &["sim", "nope", "--local"]);
        assert!(String::from_utf8_lossy(&unknown.stderr).starts_with("error: no scenario `nope`"));
    }
}

/// The variables `knock()` sets, as `knock_core::paths` reads them.
fn test_env(dir: &std::path::Path, key: &str) -> Option<std::ffi::OsString> {
    matches!(key, "APPDATA" | "XDG_CONFIG_HOME").then(|| dir.as_os_str().to_owned())
}
