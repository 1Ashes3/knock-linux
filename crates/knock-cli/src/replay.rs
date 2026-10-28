//! `knock replay` (SPEC.md §8.8, §12.1 Replay): feeds a fixture through the
//! adapter and engine on a virtual clock and compares the result with
//! `<name>.expected.json`. Any difference fails, and so does a Needs-you
//! entry without its `Notify`.
//!
//! Clock: a tick every whole second from 1000 ms; an event at the same ms as
//! a tick goes first. Every fixture runs on to its last event + 6 h + 60 s,
//! past every timer (stall, escalation, the 60 s fade, the 6 h removal).
//!
//! `knock sim --local` plays its scenarios through the same [`play`].

use knock_core::{Adapter, ClaudeCode, Effect, Engine, Input, Settings, State, Tier};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// Fixture times are relative; this puts them in 2026 so `ts` reads sensibly.
const BASE: u64 = 1_792_000_000_000;
const TAIL_MS: u64 = 6 * 3_600_000 + 60_000;

/// Settings per fixture. Non-default ones exist only to cross a timer inside
/// a recording: a 1-minute escalation (a real setting) for the three long
/// waits, and a 5 s stall with stall notifications on for `permission-fast`.
fn settings_for(name: &str) -> Settings {
    let mut s = Settings::default();
    match name {
        "deny-then-wait" | "esc-then-wait" | "closed-while-waiting" => s.escalation_ms = 60_000,
        "permission-fast" => {
            s.stall_ms = 5_000;
            s.notify_on_stall = true;
        }
        _ => {}
    }
    s
}

/// One line of a fixture or a scenario.
pub enum Step {
    /// A §8.8 hook line; `body` is its payload.
    Hook {
        t: u64,
        hook: String,
        session: String,
        body: Vec<u8>,
    },
    /// Scenarios only: `{"tMs", "control": "process_gone", "session"}`, the
    /// session's process went away (§8.4 ProcessGone).
    ProcessGone { t: u64, session: String },
}

impl Step {
    pub fn t(&self) -> u64 {
        match self {
            Step::Hook { t, .. } | Step::ProcessGone { t, .. } => *t,
        }
    }
}

/// Parses every line. Errors carry a line number, never content.
pub fn steps(text: &str) -> Result<Vec<Step>, String> {
    let mut prev = 0;
    text.lines()
        .enumerate()
        .map(|(n, line)| {
            let bad = || format!("line {}: not a fixture or scenario line", n + 1);
            let v: Value = serde_json::from_str(line).map_err(|_| bad())?;
            let t = v["tMs"].as_u64().filter(|t| *t >= prev).ok_or_else(bad)?;
            prev = t;
            let text = |v: &Value| v.as_str().map(str::to_string).ok_or_else(bad);
            if v.get("control").is_some() {
                if v["control"] != "process_gone" {
                    return Err(bad());
                }
                let session = text(&v["session"])?;
                return Ok(Step::ProcessGone { t, session });
            }
            Ok(Step::Hook {
                t,
                hook: text(&v["hook"])?,
                session: text(&v["payload"]["session_id"])?,
                body: serde_json::to_vec(&v["payload"]).map_err(|_| bad())?,
            })
        })
        .collect()
}

/// Runs one fixture; returns the `{timeline, effects}` it produced.
pub fn run(name: &str, fixture: &str) -> Result<Value, String> {
    play(settings_for(name), &steps(fixture)?, false).map(|(out, _)| out)
}

/// Plays `steps` on the virtual clock. Returns `{timeline, effects}` and the
/// most sessions alive at once.
pub fn play(settings: Settings, steps: &[Step], simulated: bool) -> Result<(Value, usize), String> {
    let adapter = ClaudeCode { users: Vec::new() };
    let mut engine = Engine::new(settings);
    let (mut timeline, mut effects) = (Vec::new(), Vec::new());
    let mut seen: Vec<(String, State)> = Vec::new();
    let mut live = 0;
    let mut record = |engine: &Engine, t: u64, fx: Vec<Effect>| {
        live = live.max(engine.sessions().len());
        for s in engine.sessions() {
            if !seen.contains(&(s.id.clone(), s.state)) {
                timeline.push(json!({"tMs": t, "sessionId": s.id, "state": s.state.as_str(), "tier": s.state.tier().as_str()}));
            }
        }
        seen = engine
            .sessions()
            .iter()
            .map(|s| (s.id.clone(), s.state))
            .collect();
        effects.extend(fx.into_iter().map(|e| match e {
            Effect::Notify { session_id, reason } => {
                json!({"tMs": t, "type": "Notify", "sessionId": session_id, "reason": reason.as_str()})
            }
            Effect::Escalate { session_id } => json!({"tMs": t, "type": "Escalate", "sessionId": session_id}),
            Effect::ReadyBatchAdd { session_id } => {
                json!({"tMs": t, "type": "ReadyBatchAdd", "sessionId": session_id})
            }
            Effect::Persist => json!({"tMs": t, "type": "Persist"}),
        }));
    };
    let mut clock = 1000;
    let mut end = 0;
    for (n, step) in steps.iter().enumerate() {
        let t = step.t();
        while clock < t {
            let fx = engine.step(Input::Tick {
                now: BASE + clock,
                suppressed: false,
            });
            record(&engine, clock, fx);
            clock += 1000;
        }
        let input = match step {
            Step::Hook { body, .. } => adapter
                .parse(body, BASE + t)
                .map_err(|e| format!("line {}: {e:?}", n + 1))?
                .map(|event| Input::Event { event, simulated }),
            Step::ProcessGone { session, .. } => Some(Input::ProcessGone {
                session_id: session.clone(),
                now: BASE + t,
            }),
        };
        if let Some(input) = input {
            let fx = engine.step(input);
            record(&engine, t, fx);
        }
        end = t;
    }
    while clock <= end + TAIL_MS {
        let fx = engine.step(Input::Tick {
            now: BASE + clock,
            suppressed: false,
        });
        record(&engine, clock, fx);
        clock += 1000;
    }
    Ok((json!({"timeline": timeline, "effects": effects}), live))
}

/// Zero missed Needs-you (§6): every entry into Needs you has its `Notify`
/// at the same time, except a stall with stall notifications off.
pub fn missed_needs_you(notify_stall: bool, out: &Value) -> Result<(), String> {
    let effects = out["effects"].as_array().map_or(&[][..], Vec::as_slice);
    for e in out["timeline"].as_array().map_or(&[][..], Vec::as_slice) {
        let stalled = e["state"] == State::Stalled.as_str();
        if e["tier"] != Tier::NeedsYou.as_str() || (stalled && !notify_stall) {
            continue;
        }
        let found = effects.iter().any(|f| {
            f["type"] == "Notify" && f["tMs"] == e["tMs"] && f["sessionId"] == e["sessionId"]
        });
        if !found {
            return Err(format!("missed Needs-you: {e}"));
        }
    }
    Ok(())
}

fn diff(what: &str, want: &Value, got: &Value) -> Result<(), String> {
    let (w, g) = (want.as_array(), got.as_array());
    let (w, g) = (
        w.map_or(&[][..], Vec::as_slice),
        g.map_or(&[][..], Vec::as_slice),
    );
    for i in 0..w.len().max(g.len()) {
        if w.get(i) != g.get(i) {
            let show = |v: Option<&Value>| v.map_or("(none)".into(), Value::to_string);
            return Err(format!(
                "{what} entry {}: expected {}, got {}",
                i + 1,
                show(w.get(i)),
                show(g.get(i))
            ));
        }
    }
    Ok(())
}

/// `knock replay <fixture.jsonl>`: compares with the `.expected.json` next to it.
pub fn one(path: &Path) -> Result<String, String> {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let name = file.strip_suffix(".jsonl").ok_or("not a .jsonl fixture")?;
    let read =
        |p: &Path| fs::read_to_string(p).map_err(|e| format!("{name}: cannot read: {}", e.kind()));
    let fixture = read(path)?;
    let want: Value = serde_json::from_str(&read(
        &path.with_file_name(format!("{name}.expected.json")),
    )?)
    .map_err(|_| format!("{name}: expected file is not JSON"))?;
    let got = run(name, &fixture).map_err(|e| format!("{name}: {e}"))?;
    missed_needs_you(settings_for(name).notify_on_stall, &got)
        .map_err(|e| format!("{name}: {e}"))?;
    diff("timeline", &want["timeline"], &got["timeline"]).map_err(|e| format!("{name}: {e}"))?;
    diff("effects", &want["effects"], &got["effects"]).map_err(|e| format!("{name}: {e}"))?;
    let count = |k: &str| got[k].as_array().map_or(0, Vec::len);
    Ok(format!(
        "{name}: ok ({} timeline entries, {} effects)",
        count("timeline"),
        count("effects")
    ))
}

/// `knock replay <dir>`: every `*.jsonl` in `dir`. Returns one line per fixture.
pub fn all(dir: &Path) -> Result<Vec<String>, String> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read: {}", e.kind()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err("no fixtures found".into());
    }
    paths.iter().map(|p| one(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    /// §12.1 Replay: every fixture matches its hand-written expected file.
    #[test]
    fn every_fixture_replays_to_its_expected_file() {
        let lines = all(&fixtures()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(lines.len(), 17);
    }

    #[test]
    fn a_difference_or_a_missed_needs_you_fails() {
        let fixture = fs::read_to_string(fixtures().join("permission-denied.jsonl")).unwrap();
        let got = run("permission-denied", &fixture).unwrap();
        let want: Value = serde_json::from_str(
            &fs::read_to_string(fixtures().join("permission-denied.expected.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(got, want);
        // Drop the Notify: the replay must call it a missed Needs-you.
        let mut missed = got.clone();
        missed["effects"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["type"] != "Notify");
        assert!(
            missed_needs_you(false, &missed)
                .unwrap_err()
                .starts_with("missed Needs-you")
        );
        let mut other = want.clone();
        other["effects"][1]["tMs"] = json!(3734);
        assert_eq!(
            diff("effects", &other["effects"], &got["effects"]).unwrap_err(),
            r#"effects entry 2: expected {"reason":"needs_permission","sessionId":"s1","tMs":3734,"type":"Notify"}, got {"reason":"needs_permission","sessionId":"s1","tMs":3733,"type":"Notify"}"#
        );
        let mut short = want;
        short["timeline"].as_array_mut().unwrap().pop();
        assert!(diff("timeline", &short["timeline"], &got["timeline"]).is_err());
    }

    /// The clock ticks between events, so timers fire inside a recording.
    #[test]
    fn ticks_cross_timers_between_events() {
        let got = run(
            "deny-then-wait",
            &fs::read_to_string(fixtures().join("deny-then-wait.jsonl")).unwrap(),
        )
        .unwrap();
        assert!(
            got["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "Escalate" && e["tMs"] == 64000)
        );
    }
}
