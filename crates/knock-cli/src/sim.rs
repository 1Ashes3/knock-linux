//! `knock sim` (SPEC.md §12.3, FR-14): plays a scripted scenario of synthetic
//! hook events, either against the running app (§8.1, `X-Knock-Sim: 1`) or,
//! with `--local`, through knock-core on a virtual clock with no app.
//!
//! Scenarios are `scenarios/<name>.jsonl`, built into the binary. They hold
//! made-up §8.8 lines with only allow-listed fields, plus one sim-only line,
//! `{"tMs", "control": "process_gone", "session"}`. Against the app that
//! line is skipped: sim sessions have no process (§8.1), and a live way to end
//! one is a Phase 2 decision.
//!
//! Each scenario uses its own session ids, so scenarios never share a session,
//! and running one again continues its sessions instead of adding new ones.

use crate::replay::{self, Step};
use knock_core::Settings;
use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const SCENARIOS: [(&str, &str); 7] = [
    (
        "one-all-states",
        include_str!("../scenarios/one-all-states.jsonl"),
    ),
    (
        "three-knocks",
        include_str!("../scenarios/three-knocks.jsonl"),
    ),
    ("eight-den", include_str!("../scenarios/eight-den.jsonl")),
    ("stall", include_str!("../scenarios/stall.jsonl")),
    ("crash", include_str!("../scenarios/crash.jsonl")),
    ("away", include_str!("../scenarios/away.jsonl")),
    ("demo", include_str!("../scenarios/demo.jsonl")),
];

fn scenario(name: &str) -> Result<Vec<Step>, String> {
    let (_, text) = SCENARIOS.iter().find(|(n, _)| *n == name).ok_or_else(|| {
        let names: Vec<_> = SCENARIOS.iter().map(|(n, _)| *n).collect();
        format!("no scenario `{name}`; try one of: {}", names.join(", "))
    })?;
    replay::steps(text).map_err(|e| format!("{name}: {e}"))
}

/// `m:ss.mmm`
fn clock(ms: u64) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// What a local run produced: the `{timeline, effects}` and the most
/// sessions alive at once.
pub fn play_local(name: &str) -> Result<(Value, usize), String> {
    let settings = Settings::default();
    let notify_stall = settings.notify_on_stall;
    let (out, live) = replay::play(settings, &scenario(name)?, true)?;
    replay::missed_needs_you(notify_stall, &out).map_err(|e| format!("{name}: {e}"))?;
    Ok((out, live))
}

/// `knock sim <name> --local`: the state timeline and effects, in time order
/// (`Persist` left out), with default settings.
pub fn local(name: &str) -> Result<String, String> {
    let (out, live) = play_local(name)?;
    let list = |k: &str| out[k].as_array().cloned().unwrap_or_default();
    let mut lines: Vec<(u64, String)> = list("timeline")
        .iter()
        .map(|e| {
            let line = format!("{}  {} ({})", e["sessionId"], e["state"], e["tier"]);
            (e["tMs"].as_u64().unwrap_or(0), line.replace('"', ""))
        })
        .collect();
    let effects = list("effects");
    let effects: Vec<_> = effects.iter().filter(|e| e["type"] != "Persist").collect();
    for e in &effects {
        let reason = e["reason"]
            .as_str()
            .map_or(String::new(), |r| format!(" {r}"));
        let line = format!("{}  {}{reason}", e["sessionId"], e["type"]).replace('"', "");
        lines.push((e["tMs"].as_u64().unwrap_or(0), line));
    }
    lines.sort_by_key(|(t, _)| *t); // stable: states before effects at a time
    let count = |k: &str| effects.iter().filter(|e| e["type"] == k).count();
    let mut text = format!("{name} (local, virtual clock, default settings)\n");
    for (t, line) in lines {
        text += &format!("{:>10}  {line}\n", clock(t));
    }
    text += &format!(
        "{} Notify, {} Escalate, {} ReadyBatchAdd; at most {live} sessions at once; no missed Needs-you",
        count("Notify"),
        count("Escalate"),
        count("ReadyBatchAdd"),
    );
    Ok(text)
}

/// Port and token from `server.json` (§8.1). The token never leaves this
/// process except in the `X-Knock-Token` header.
fn server() -> Result<(u16, String), String> {
    let not_running = "Knock doesn't seem to be running: %APPDATA%\\Knock\\server.json \
                       was not found. Start Knock, then run this again.";
    let dir = std::env::var_os("APPDATA").ok_or(not_running)?;
    let path = PathBuf::from(dir).join("Knock").join("server.json");
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_running.into()),
        Err(e) => return Err(format!("cannot read Knock's server.json: {}", e.kind())),
    };
    let bad = || "Knock's server.json is not valid. Restart Knock, then try again.".to_string();
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    let port = v["port"].as_u64().and_then(|p| u16::try_from(p).ok());
    let token = v["token"].as_str().filter(|t| {
        // base64url only, so it can't break the header.
        !t.is_empty()
            && t.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    });
    match (port, token) {
        (Some(p), Some(t)) if p > 0 => Ok((p, t.to_string())),
        _ => Err(bad()),
    }
}

/// One hook POST to `127.0.0.1:port` (loopback only). Returns the HTTP status.
fn post(port: u16, token: &str, body: &[u8]) -> io::Result<u16> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let timeout = Duration::from_secs(2);
    // Any connect failure means nothing answered. (Windows retries a refused
    // loopback connect for about 2 s, so it often shows as a timeout.)
    let mut s = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|_| io::Error::from(io::ErrorKind::ConnectionRefused))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    write!(
        s,
        "POST /v1/hooks/claude-code HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\nX-Knock-Token: {token}\r\nX-Knock-Sim: 1\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)?;
    let mut reply = Vec::new();
    s.read_to_end(&mut reply)?;
    // "HTTP/1.1 200 OK"
    std::str::from_utf8(reply.get(9..12).unwrap_or_default())
        .ok()
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an HTTP reply"))
}

/// `knock sim <name> [--speed <factor>]`: posts each event to the running
/// app, `tMs / speed` after the start.
pub fn live(name: &str, speed: f64) -> Result<(), String> {
    if !(speed.is_finite() && speed > 0.0) {
        return Err("--speed must be a number above 0, like 2 or 0.5".into());
    }
    let steps = scenario(name)?;
    let (port, token) = server()?;
    let last = steps.last().map_or(0, Step::t);
    println!(
        "Playing {name} against Knock on 127.0.0.1:{port}: {} lines over {} at speed {speed}",
        steps.len(),
        clock((last as f64 / speed) as u64)
    );
    let start = Instant::now();
    for step in &steps {
        let due = Duration::from_secs_f64(step.t() as f64 / 1000.0 / speed);
        std::thread::sleep(due.saturating_sub(start.elapsed()));
        let at = clock(step.t());
        match step {
            Step::ProcessGone { session, .. } => println!(
                "{at:>10}  {session}  skipped process_gone: sim sessions have no process \
                 in the app yet. Run with --local to see it."
            ),
            Step::Hook {
                hook,
                session,
                body,
                ..
            } => match post(port, &token, body) {
                Ok(200) => println!("{at:>10}  {session}  {hook}"),
                Ok(401) => {
                    return Err(
                        "Knock refused the request: the token in server.json doesn't \
                                match the running app. Restart Knock, then try again."
                            .into(),
                    );
                }
                Ok(code) => return Err(format!("Knock answered {code} at {at}")),
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                    return Err(format!(
                        "Knock isn't running: nothing answered on 127.0.0.1:{port}. \
                         Start Knock, then run this again."
                    ));
                }
                Err(e) => {
                    return Err(format!(
                        "lost the connection to Knock at {at}: {}",
                        e.kind()
                    ));
                }
            },
        }
    }
    println!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use std::collections::HashSet;

    const MIN: u64 = 60_000;

    fn timeline(out: &Value) -> Vec<(u64, String, String)> {
        let list = out["timeline"].as_array().unwrap();
        list.iter()
            .map(|e| {
                let s = |k: &str| e[k].as_str().unwrap().to_string();
                (e["tMs"].as_u64().unwrap(), s("sessionId"), s("state"))
            })
            .collect()
    }

    fn states(out: &Value) -> HashSet<String> {
        timeline(out).into_iter().map(|(_, _, s)| s).collect()
    }

    fn effects(out: &Value, kind: &str) -> usize {
        let list = out["effects"].as_array().unwrap();
        list.iter().filter(|e| e["type"] == kind).count()
    }

    /// Most sessions in `needs_input` at the same time.
    fn overlapping_needs(out: &Value) -> usize {
        let mut now = HashSet::new();
        let mut most = 0;
        for (_, id, state) in timeline(out) {
            if state == "needs_input" {
                now.insert(id);
            } else {
                now.remove(&id);
            }
            most = most.max(now.len());
        }
        most
    }

    /// Same discipline as fixtures (§8.8): hook lines pass the fixture check,
    /// control lines are exactly the one sim-only shape.
    #[test]
    fn scenarios_hold_only_sanitized_fixture_lines_and_controls() {
        for (name, text) in SCENARIOS {
            let mut hooks = String::new();
            for line in text.lines() {
                let v: Value = serde_json::from_str(line).unwrap();
                match v.get("control") {
                    Some(c) => {
                        assert_eq!(c, "process_gone", "{name}");
                        let o = v.as_object().unwrap();
                        assert_eq!(o.len(), 3, "{name}: control line keys");
                        assert!(o["tMs"].is_u64(), "{name}");
                        let sid = o["session"].as_str().unwrap();
                        assert!(sid.strip_prefix('s').unwrap().parse::<u32>().is_ok());
                    }
                    None => hooks += &format!("{line}\n"),
                }
            }
            fixtures::check_fixture(&hooks).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(scenario(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn scenarios_never_share_a_session() {
        let mut owner = std::collections::HashMap::new();
        for (name, _) in SCENARIOS {
            for step in scenario(name).unwrap() {
                let (Step::Hook { session, .. } | Step::ProcessGone { session, .. }) = step;
                assert_eq!(*owner.entry(session).or_insert(name), name);
            }
        }
    }

    /// FR-14 in Phase 1: every §12.3 scenario, locally, with zero missed
    /// Needs-you (checked inside `play_local`).
    #[test]
    fn one_all_states_reaches_every_v1_state() {
        let (out, live) = play_local("one-all-states").unwrap();
        let want = [
            "needs_input",
            "failed",
            "stalled",
            "ready",
            "working",
            "idle",
            "ended_unexpectedly",
        ];
        assert_eq!(states(&out), want.map(String::from).into());
        assert_eq!(live, 1);
    }

    #[test]
    fn three_knocks_overlap_with_exactly_three_notify() {
        let (out, live) = play_local("three-knocks").unwrap();
        assert_eq!((live, overlapping_needs(&out)), (3, 3));
        assert_eq!(effects(&out, "Notify"), 3);
        assert_eq!(effects(&out, "Escalate"), 0);
    }

    #[test]
    fn eight_den_has_eight_sessions_at_once_in_every_tier() {
        let (out, live) = play_local("eight-den").unwrap();
        assert_eq!(live, 8);
        for s in ["needs_input", "failed", "ready", "working"] {
            assert!(states(&out).contains(s), "{s}");
        }
        assert!(overlapping_needs(&out) >= 2, "more than one steps out");
    }

    #[test]
    fn stall_stalls_and_recovers() {
        let (out, _) = play_local("stall").unwrap();
        let t = timeline(&out);
        let stalled = t.iter().position(|(_, _, s)| s == "stalled").unwrap();
        assert_eq!(t[stalled].0, 10 * MIN + 5000, "10 min after the last event");
        let after: Vec<_> = t[stalled + 1..]
            .iter()
            .map(|(_, _, s)| s.as_str())
            .collect();
        assert_eq!(after, ["working", "ready"]);
        assert_eq!(
            effects(&out, "Notify"),
            0,
            "notifyOnStall is off by default"
        );
    }

    #[test]
    fn crash_ends_unexpectedly() {
        let (out, _) = play_local("crash").unwrap();
        let t = timeline(&out);
        assert_eq!(t.last().unwrap().2, "ended_unexpectedly");
        assert_eq!(effects(&out, "Notify"), 0);
    }

    #[test]
    fn away_has_ready_failed_and_needs_you_within_fifteen_minutes() {
        let (out, _) = play_local("away").unwrap();
        let steps = scenario("away").unwrap();
        assert!(steps.last().unwrap().t() <= 15 * MIN);
        for s in ["ready", "failed", "needs_input"] {
            let first = timeline(&out).into_iter().find(|(_, _, st)| st == s);
            assert!(first.unwrap().0 <= 15 * MIN, "{s}");
        }
    }

    /// FR-9 (ADR 0034) on the virtual clock: the user leaves as `away` starts
    /// and comes back 1 s after its last event. Every session is simulated
    /// and counts; each is listed with its state at return.
    #[test]
    fn away_summarizes_every_session_on_return() {
        use knock_core::{AWAY_MS, Adapter, Away, ClaudeCode, Engine, Input};
        const BASE: u64 = 1_792_000_000_000;
        let adapter = ClaudeCode { users: Vec::new() };
        let mut engine = Engine::new(Settings::default());
        let mut away = Away::new(AWAY_MS);
        let steps = scenario("away").unwrap();
        let back = steps.last().unwrap().t() + 1000;
        let (mut next, mut got) = (steps.iter().peekable(), Vec::new());
        for clock in (0..=back).step_by(1000) {
            // Events at the same ms as a tick go first, as in a replay.
            while let Some(step) = next.next_if(|s| s.t() <= clock) {
                let Step::Hook { body, t, .. } = step else {
                    continue;
                };
                let event = adapter.parse(body, BASE + t).unwrap().unwrap();
                engine.step(Input::Event {
                    event,
                    simulated: true,
                });
            }
            engine.step(Input::Tick { now: BASE + clock });
            let idle = if clock < back { clock } else { 0 };
            if let Some(s) = away.tick(BASE + clock, idle, engine.sessions()) {
                got.push((clock, s));
            }
        }
        assert_eq!(got.len(), 1, "{got:?}");
        let (at, s) = &got[0];
        assert_eq!((*at, s.since), (back, BASE));
        let items: Vec<_> = (s.items.iter())
            .map(|i| (i.id.as_str(), i.project.as_str(), i.outcome.as_str()))
            .collect();
        assert_eq!(
            items,
            [
                ("s51", "web-shop", "ready"),
                ("s52", "api-gateway", "failed"),
                ("s53", "data-jobs", "needs_you"),
                ("s54", "docs-site", "ready"),
            ]
        );
    }

    #[test]
    fn demo_is_a_short_mix() {
        let (out, live) = play_local("demo").unwrap();
        assert!(scenario("demo").unwrap().last().unwrap().t() <= 60_000);
        assert_eq!(live, 4);
        for s in ["idle", "working", "needs_input", "failed", "ready"] {
            assert!(states(&out).contains(s), "{s}");
        }
        assert_eq!(effects(&out, "Notify"), 3);
        assert_eq!(effects(&out, "Escalate"), 0);
    }

    #[test]
    fn local_prints_the_timeline_and_effects() {
        let text = local("crash").unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], "crash (local, virtual clock, default settings)");
        assert_eq!(lines[1], "  0:00.000  s41  working (working)");
        assert_eq!(lines[2], "  0:08.000  s41  ended_unexpectedly (resting)");
        assert_eq!(
            lines[3],
            "0 Notify, 0 Escalate, 0 ReadyBatchAdd; at most 1 sessions at once; no missed Needs-you"
        );
        assert!(
            local("three-knocks")
                .unwrap()
                .contains("s11  Notify needs_permission")
        );
        assert!(local("nope").unwrap_err().starts_with("no scenario `nope`"));
    }
}
