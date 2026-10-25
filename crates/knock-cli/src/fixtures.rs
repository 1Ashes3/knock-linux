//! `knock fixtures sanitize` (SPEC.md §8.3, §8.8, §11.3; ADR 0008, 0009).
//!
//! Turns a raw Phase 0 recording into sanitized fixtures. Raw content is never
//! printed: every error carries a line number or a count, never a value.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::fs;
use std::path::Path;

/// Payload fields kept (ADR 0009). Everything else is dropped.
const ALLOW: [&str; 7] = [
    "session_id",
    "cwd",
    "hook_event_name",
    "tool_name",
    "notification_type",
    "error",
    "agent_id",
];
const CWD_PREFIX: &str = "C:\\fixture\\";

fn charset(s: &str, ok: impl Fn(char) -> bool) -> bool {
    (1..=64).contains(&s.len()) && s.chars().all(ok)
}
fn is_name(s: &str) -> bool {
    charset(s, |c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}
fn is_code(s: &str) -> bool {
    charset(s, |c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
    })
}
fn is_project(s: &str) -> bool {
    charset(s, |c| c.is_ascii_alphanumeric() || "_.-".contains(c)) && s != "." && s != ".."
}
fn is_fake(s: &str, prefix: char) -> bool {
    s.strip_prefix(prefix)
        .is_some_and(|n| charset(n, |c| c.is_ascii_digit()))
}
fn is_fixture_name(s: &str) -> bool {
    charset(s, |c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
    })
}
/// Path or user-name markers. False positives are fine; leaks are not.
fn looks_like_path(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    let b = l.as_bytes();
    l.contains("users")
        || l.contains("appdata")
        || l.contains('\\')
        || l.contains('/')
        || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// The one rule both the sanitizer and the checker use for a kept value.
fn safe(s: &str, ok: fn(&str) -> bool) -> bool {
    ok(s) && !looks_like_path(s)
}

fn keep(s: Option<&str>, ok: fn(&str) -> bool) -> String {
    s.filter(|s| safe(s, ok)).unwrap_or("other").to_string()
}

/// `cwd` parts with `.` dropped and `..` applied.
fn parts(cwd: &str) -> Vec<&str> {
    let mut v = Vec::new();
    for p in cwd.split(['\\', '/']) {
        match p {
            "" | "." => {}
            ".." => {
                v.pop();
            }
            _ => v.push(p),
        }
    }
    v
}

/// `%USERNAME%` and the last part of `%USERPROFILE%`, read once per run.
fn env_users() -> Vec<String> {
    ["USERNAME", "USERPROFILE"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .filter_map(|v| parts(&v).last().map(|s| s.to_string()))
        .collect()
}

/// Last `cwd` part; `home` for a user's home folder (directly under `Users`,
/// or named like a `users` entry); `project` when the part isn't safe.
fn project_name<'a>(cwd: &'a str, users: &[String]) -> &'a str {
    let p = parts(cwd);
    let last = p.last().copied().unwrap_or("");
    let parent = p.len().checked_sub(2).map_or("", |i| p[i]);
    let lower = last.to_lowercase();
    if parent.eq_ignore_ascii_case("users") || users.iter().any(|u| u.to_lowercase() == lower) {
        "home"
    } else if safe(last, is_project) {
        last
    } else {
        "project"
    }
}

/// JSON parse that fails on a repeated key at any level: `Value` keeps only
/// the last one, so a fake value could hide a real one. Never echoes content.
fn parse_strict(s: &str) -> Result<Value, String> {
    serde_json::from_str::<Strict>(s)
        .map(|v| v.0)
        // The visitor's only data error is the repeated key.
        .map_err(|e| {
            if e.is_data() {
                "repeated key"
            } else {
                "not JSON"
            }
            .into()
        })
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON")
    }
    fn visit_bool<E: de::Error>(self, b: bool) -> Result<Value, E> {
        Ok(b.into())
    }
    fn visit_i64<E: de::Error>(self, n: i64) -> Result<Value, E> {
        Ok(n.into())
    }
    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Value, E> {
        Ok(n.into())
    }
    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Value, E> {
        Ok(n.into())
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<Value, E> {
        Ok(s.into())
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut v = Vec::new();
        while let Some(Strict(x)) = a.next_element()? {
            v.push(x);
        }
        Ok(v.into())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut m = Map::new();
        while let Some(k) = a.next_key::<String>()? {
            if m.contains_key(&k) {
                return Err(de::Error::custom("repeated key"));
            }
            let Strict(v) = a.next_value()?;
            m.insert(k, v);
        }
        Ok(m.into())
    }
}

struct Raw {
    /// Line number in the recording file.
    n: usize,
    t: i64,
    hook: Option<String>,
    payload: Map<String, Value>,
}

fn parse_raw(n: usize, line: &str) -> Result<Raw, String> {
    let bad = || format!("recording line {n}: not a recorder line");
    let v: Value = serde_json::from_str(line).map_err(|_| bad())?;
    let Value::Object(mut o) = v else {
        return Err(bad());
    };
    let t = o.get("tMs").and_then(Value::as_i64).ok_or_else(bad)?;
    let hook = o.get("hook").and_then(Value::as_str).map(str::to_string);
    let Some(Value::Object(payload)) = o.remove("payload") else {
        return Err(bad());
    };
    Ok(Raw {
        n,
        t,
        hook,
        payload,
    })
}

/// Splits events into sessions in first-event order. An event that arrives after its
/// session's `SessionEnd` starts a new segment (`claude --continue` reuses the id).
fn segments(raw: &[Raw]) -> Result<Vec<Vec<usize>>, String> {
    let mut open: HashMap<&str, usize> = HashMap::new();
    let mut segs: Vec<Vec<usize>> = Vec::new();
    for (i, r) in raw.iter().enumerate() {
        let sid = r
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("recording line {}: no session_id", r.n))?;
        let seg = *open.entry(sid).or_insert_with(|| {
            segs.push(Vec::new());
            segs.len() - 1
        });
        segs[seg].push(i);
        if r.hook.as_deref() == Some("SessionEnd") {
            open.remove(sid);
        }
    }
    Ok(segs)
}

fn fake(ids: &mut HashMap<String, String>, prefix: char, v: &Value) -> String {
    let n = ids.len() + 1;
    ids.entry(v.to_string())
        .or_insert_with(|| format!("{prefix}{n}"))
        .clone()
}

/// One fixture from raw events (in recording order). Keeps only the allow-list.
fn sanitize(events: &[&Raw], users: &[String]) -> String {
    let t0 = events[0].t;
    let (mut sids, mut aids) = (HashMap::new(), HashMap::new());
    let mut out = String::new();
    for r in events {
        let mut p = Map::new();
        for (k, v) in &r.payload {
            let s = v.as_str();
            let clean = match k.as_str() {
                "session_id" => fake(&mut sids, 's', v),
                "agent_id" => fake(&mut aids, 'a', v),
                "cwd" => format!(
                    "{CWD_PREFIX}{}",
                    s.map_or("project", |c| project_name(c, users))
                ),
                "hook_event_name" | "tool_name" => keep(s, is_name),
                "error" | "notification_type" => keep(s, is_code),
                _ => continue,
            };
            p.insert(k.clone(), Value::String(clean));
        }
        let hook = Value::String(keep(r.hook.as_deref(), is_name));
        let _ = writeln!(
            out,
            "{{\"tMs\":{},\"hook\":{hook},\"payload\":{}}}",
            r.t - t0,
            Value::Object(p)
        );
    }
    out
}

/// Checks one fixture line against §8.8 and the sanitization rules. Returns `tMs`.
fn check_line(line: &str) -> Result<u64, String> {
    let v = parse_strict(line)?;
    let o = v.as_object().ok_or("not an object")?;
    if o.len() != 3 {
        return Err("top level must be exactly tMs, hook, payload".into());
    }
    let t = o
        .get("tMs")
        .and_then(Value::as_u64)
        .ok_or("tMs missing or negative")?;
    let hook = o
        .get("hook")
        .and_then(Value::as_str)
        .ok_or("hook missing")?;
    let p = o
        .get("payload")
        .and_then(Value::as_object)
        .ok_or("payload missing")?;
    if !safe(hook, is_name) {
        return Err("hook is not a hook name".into());
    }
    if p.get("hook_event_name").and_then(Value::as_str) != Some(hook) {
        return Err("hook_event_name differs from hook".into());
    }
    for (k, v) in p {
        if !ALLOW.contains(&k.as_str()) {
            return Err("payload key outside the allow-list".into());
        }
        let s = v.as_str().ok_or_else(|| format!("`{k}` is not a string"))?;
        let ok = match k.as_str() {
            "session_id" => is_fake(s, 's'),
            "agent_id" => is_fake(s, 'a'),
            "cwd" => s
                .strip_prefix(CWD_PREFIX)
                .is_some_and(|x| safe(x, is_project)),
            "error" | "notification_type" => safe(s, is_code),
            _ => safe(s, is_name),
        };
        if !ok {
            return Err(format!("`{k}` value is not sanitized"));
        }
    }
    Ok(t)
}

/// Checks a whole fixture: every line, first `tMs` is 0, `tMs` never goes back.
fn check_fixture(text: &str) -> Result<(), String> {
    let mut prev = None;
    for (i, line) in text.lines().enumerate() {
        let t = check_line(line).map_err(|e| format!("line {}: {e}", i + 1))?;
        match prev {
            None if t != 0 => return Err("line 1: tMs must be 0".into()),
            Some(p) if t < p => return Err(format!("line {}: tMs goes back", i + 1)),
            _ => prev = Some(t),
        }
    }
    prev.map(|_| ()).ok_or_else(|| "empty fixture".into())
}

/// §8.4 names allowed in an `*.expected.json`.
const STATES: [&str; 12] = [
    "needs_input",
    "failed",
    "stalled",
    "blocked_fixable",
    "ready",
    "working",
    "waiting_agent",
    "communicating",
    "idle",
    "ended_unexpectedly",
    "paused",
    "cancelled",
];
const TIERS: [&str; 4] = ["needs_you", "ready", "working", "resting"];
const EFFECTS: [&str; 4] = ["Notify", "Escalate", "ReadyBatchAdd", "Persist"];
const REASONS: [&str; 4] = ["needs_permission", "needs_answer", "failed", "stalled"];

type Rule = (&'static str, fn(&Value) -> bool);
const T_MS: Rule = ("tMs", Value::is_u64);
const SID: Rule = ("sessionId", |v| v.as_str().is_some_and(|s| is_fake(s, 's')));

fn one_of(v: &Value, set: &[&str]) -> bool {
    v.as_str().is_some_and(|s| set.contains(&s))
}

/// An object with exactly the `rules` keys, each value passing its rule.
fn exact(v: &Value, rules: &[Rule]) -> bool {
    v.as_object().is_some_and(|o| {
        o.len() == rules.len() && rules.iter().all(|(k, ok)| o.get(*k).is_some_and(ok))
    })
}

/// Checks an `*.expected.json` against the exact §8.8 shape with §8.4 names:
/// no other keys, no free strings, every `sessionId` a fake id.
fn check_expected(text: &str) -> Result<(), String> {
    let v = parse_strict(text)?;
    let top = || "top level must be exactly timeline and effects arrays".to_string();
    let o = v.as_object().filter(|o| o.len() == 2).ok_or_else(top)?;
    let list = |k: &str| o.get(k).and_then(Value::as_array).ok_or_else(top);
    let (timeline, effects) = (list("timeline")?, list("effects")?);
    let entry: [Rule; 4] = [
        T_MS,
        SID,
        ("state", |v| one_of(v, &STATES)),
        ("tier", |v| one_of(v, &TIERS)),
    ];
    for (i, e) in timeline.iter().enumerate() {
        if !exact(e, &entry) {
            return Err(format!(
                "timeline entry {}: not tMs, sessionId, state, tier",
                i + 1
            ));
        }
    }
    for (i, e) in effects.iter().enumerate() {
        let mut rules: Vec<Rule> = vec![T_MS, ("type", |v| one_of(v, &EFFECTS))];
        match e.get("type").and_then(Value::as_str) {
            Some("Notify") => rules.extend([SID, ("reason", |v| one_of(v, &REASONS))]),
            Some("Persist") if e.get("sessionId").is_none() => {}
            _ => rules.push(SID),
        }
        if !exact(e, &rules) {
            return Err(format!("effect {}: not a §8.4 effect", i + 1));
        }
    }
    Ok(())
}

/// `knock fixtures check <dir>`: every file under `dir`, recursively, must be a
/// sanitized `*.jsonl` fixture or `*.expected.json`; any other file fails.
/// Returns the number of files checked.
pub fn check(dir: &Path) -> Result<usize, String> {
    let io = |e: std::io::Error| format!("cannot read: {}", e.kind());
    let mut n = 0;
    for entry in fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.is_dir() {
            n += check(&path)?;
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // Case-sensitive on purpose: `x.JSONL` is a bad name, not a skipped one.
        let checker: fn(&str) -> Result<(), String> = if name.ends_with(".expected.json") {
            check_expected
        } else if name.ends_with(".jsonl") {
            check_fixture
        } else {
            return Err(format!("{name}: not a .jsonl or .expected.json file"));
        };
        let text = fs::read_to_string(&path).map_err(|e| format!("{name}: {}", io(e)))?;
        checker(&text).map_err(|e| format!("{name}: {e}"))?;
        n += 1;
    }
    Ok(n)
}

/// Pure core of the command: `names` has one entry per session segment, in
/// first-event order; `-` discards a segment, a repeated name merges segments.
fn sanitize_recording(
    text: &str,
    names: &[&str],
    users: &[String],
) -> Result<Vec<(String, String)>, String> {
    let raw = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| parse_raw(i + 1, l))
        .collect::<Result<Vec<_>, _>>()?;
    let segs = segments(&raw)?;
    if segs.len() != names.len() {
        return Err(format!(
            "recording has {} sessions, map has {} names",
            segs.len(),
            names.len()
        ));
    }
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (name, seg) in names.iter().zip(segs) {
        if *name == "-" {
            continue;
        }
        if !is_fixture_name(name) {
            return Err(format!("bad fixture name `{name}`: use a-z, 0-9 and -"));
        }
        match groups.iter_mut().find(|(n, _)| n == name) {
            Some((_, idx)) => idx.extend(seg),
            None => groups.push((name, seg)),
        }
    }
    groups
        .into_iter()
        .map(|(name, mut idx)| {
            idx.sort_unstable();
            let text = sanitize(&idx.iter().map(|&i| &raw[i]).collect::<Vec<_>>(), users);
            check_fixture(&text).map_err(|e| format!("{name}: {e}"))?;
            Ok((name.to_string(), text))
        })
        .collect()
}

/// `knock fixtures sanitize <recording.jsonl> <out-dir> --map <names-file>`.
pub fn run(recording: &Path, out_dir: &Path, map: &Path) -> Result<(), String> {
    let text = fs::read_to_string(recording)
        .map_err(|e| format!("cannot read recording: {}", e.kind()))?;
    let map = fs::read_to_string(map).map_err(|e| format!("cannot read map: {}", e.kind()))?;
    let names: Vec<&str> = map
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for (name, body) in sanitize_recording(&text, &names, &env_users())? {
        fs::write(out_dir.join(format!("{name}.jsonl")), &body)
            .map_err(|e| format!("cannot write {name}.jsonl: {}", e.kind()))?;
        println!("{name}.jsonl: {} lines", body.lines().count());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic raw recording. Free text and paths are made up for the test.
    const RAW: &str = r#"
{"tMs":1000,"hook":"UserPromptSubmit","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"UserPromptSubmit","prompt":"SECRET_PROMPT","transcript_path":"C:\\Users\\someone\\AppData\\t.jsonl","permission_mode":"default"}}
{"tMs":1500,"hook":"PreToolUse","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"C:\\Users\\someone\\x.txt","content":"SECRET_CONTENT"}}}
{"tMs":1600,"hook":"PreToolUse","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"PreToolUse","tool_name":"C:\\Users\\someone\\evil","agent_id":"agent-xyz","agent_transcript_path":"C:\\Users\\someone\\a.jsonl"}}
{"tMs":2000,"hook":"Notification","payload":{"session_id":"real-2","cwd":"D:\\work\\other","hook_event_name":"Notification","notification_type":"permission_prompt","message":"SECRET_MESSAGE"}}
{"tMs":2500,"hook":"StopFailure","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"StopFailure","error":"Some free text error","last_assistant_message":"SECRET_REPLY"}}
{"tMs":2600,"hook":"StopFailure","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"StopFailure","error":"rate_limit"}}
{"tMs":3000,"hook":"SessionEnd","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"SessionEnd","reason":"other"}}
{"tMs":9000,"hook":"UserPromptSubmit","payload":{"session_id":"real-1","cwd":"C:\\Users\\someone\\code\\demo","hook_event_name":"UserPromptSubmit","prompt":"SECRET_AGAIN"}}
"#;

    fn lines(text: &str) -> Vec<Value> {
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn drops_free_text_and_paths() {
        let out = sanitize_recording(RAW, &["mixed", "mixed", "continued"], &[]).unwrap();
        assert_eq!(out.len(), 2);
        for (_, body) in &out {
            for bad in [
                "SECRET",
                "Users",
                "someone",
                "AppData",
                "real-",
                "agent-xyz",
                "D:",
            ] {
                assert!(!body.contains(bad), "leaked {bad}");
            }
            for bad in [
                "\"prompt\"",
                "tool_input",
                "transcript",
                "message",
                "permission_mode",
                "reason",
            ] {
                assert!(!body.contains(bad), "kept key {bad}");
            }
            check_fixture(body).unwrap();
        }
        let mixed = lines(&out[0].1);
        assert_eq!(mixed.len(), 7);
        assert_eq!(mixed[0]["tMs"], 0);
        assert_eq!(mixed[6]["tMs"], 2000);
        assert_eq!(mixed[0]["payload"]["cwd"], "C:\\fixture\\demo");
        assert_eq!(mixed[0]["payload"]["session_id"], "s1");
        assert_eq!(mixed[2]["payload"]["tool_name"], "other");
        assert_eq!(mixed[2]["payload"]["agent_id"], "a1");
        assert_eq!(mixed[3]["payload"]["session_id"], "s2");
        assert_eq!(mixed[3]["payload"]["cwd"], "C:\\fixture\\other");
        assert_eq!(mixed[4]["payload"]["error"], "other");
        assert_eq!(mixed[5]["payload"]["error"], "rate_limit");
        assert_eq!(mixed[5]["payload"]["session_id"], "s1");
        // The event after SessionEnd is its own segment, with tMs restarting at 0.
        let cont = lines(&out[1].1);
        assert_eq!(out[1].0, "continued");
        assert_eq!(cont.len(), 1);
        assert_eq!(cont[0]["tMs"], 0);
        assert_eq!(cont[0]["payload"]["session_id"], "s1");
    }

    #[test]
    fn discard_and_errors_never_echo_content() {
        let out = sanitize_recording(RAW, &["-", "two", "-"], &[]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(lines(&out[0].1).len(), 1);
        let e = sanitize_recording(RAW, &["one"], &[]).unwrap_err();
        assert_eq!(e, "recording has 3 sessions, map has 1 names");
        let e = sanitize_recording("{\"tMs\":1,\"payload\":\"SECRET\"}", &["x"], &[]).unwrap_err();
        assert!(!e.contains("SECRET"));
        assert!(sanitize_recording(RAW, &["../x", "a", "b"], &[]).is_err());
    }

    #[test]
    fn check_rejects_unsanitized_lines() {
        let ok = r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","cwd":"C:\\fixture\\p","hook_event_name":"Stop"}}"#;
        check_line(ok).unwrap();
        for bad in [
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","prompt":"hi"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","cwd":"C:\\Users\\p"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","cwd":"C:\\fixture\\a\\b"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"0b1f-real","hook_event_name":"Stop"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","agent_id":"s2","hook_event_name":"Stop"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","tool_name":"D:"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","tool_name":"AppData"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","error":"Bad Thing"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop","error":{"x":1}}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Other"}}"#,
            r#"{"tMs":-1,"hook":"Stop","payload":{"hook_event_name":"Stop"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"hook_event_name":"Stop"},"extra":1}"#,
            r#"{"tMs":0,"payload":{}}"#,
            "not json",
        ] {
            assert!(check_line(bad).is_err(), "accepted {bad}");
        }
        assert!(check_fixture(&format!("{ok}\n{}", ok.replace("\"tMs\":0", "\"tMs\":5"))).is_ok());
        assert!(check_fixture(&ok.replace("\"tMs\":0", "\"tMs\":5")).is_err());
        assert!(check_fixture("").is_err());
    }

    #[test]
    fn check_rejects_repeated_keys() {
        // A real value hidden behind a fake one: `Value` would keep the last.
        for bad in [
            r#"{"tMs":0,"hook":"StopFailure","payload":{"session_id":"s1","cwd":"C:\\Users\\alice\\code","cwd":"C:\\fixture\\p","hook_event_name":"StopFailure","error":"Some free text","error":"rate_limit"}}"#,
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","cwd":"C:\\Users\\alice\\code","cwd":"C:\\fixture\\p","hook_event_name":"Stop"}}"#,
            r#"{"tMs":0,"hook":"StopFailure","payload":{"session_id":"s1","hook_event_name":"StopFailure","error":"Some free text","error":"rate_limit"}}"#,
            r#"{"tMs":0,"hook":"Some free text","hook":"Stop","payload":{"session_id":"s1","hook_event_name":"Stop"}}"#,
            // The same key spelled with an escape.
            r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","cwd":"C:\\Users\\a","c\u0077d":"C:\\fixture\\p","hook_event_name":"Stop"}}"#,
        ] {
            assert_eq!(check_line(bad).unwrap_err(), "repeated key", "{bad}");
        }
        assert_eq!(
            check_expected(r#"{"timeline":[{"sessionId":"real-id","sessionId":"s1"}]}"#),
            Err("repeated key".into())
        );
        assert_eq!(check_line("{\"tMs\":0,"), Err("not JSON".into()));
    }

    #[test]
    fn path_like_values_become_placeholders_not_errors() {
        let raw = r#"{"tMs":0,"hook":"PreToolUse","payload":{"session_id":"x","cwd":"C:\\Users\\alice","hook_event_name":"PreToolUse","tool_name":"mcp__github__list_users"}}
{"tMs":5,"hook":"StopFailure","payload":{"session_id":"x","cwd":"D:\\src\\appdata-tools","hook_event_name":"StopFailure","error":"users_quota"}}
{"tMs":6,"hook":"Stop","payload":{"session_id":"x","cwd":"/Users/alice/","hook_event_name":"Stop"}}
{"tMs":7,"hook":"Stop","payload":{"session_id":"x","cwd":"\\\\server\\USERS\\alice","hook_event_name":"Stop"}}
{"tMs":8,"hook":"Stop","payload":{"session_id":"x","cwd":"C:\\Users\\alice\\code\\demo","hook_event_name":"Stop"}}
{"tMs":9,"hook":"Stop","payload":{"session_id":"x","cwd":"C:\\Users","hook_event_name":"Stop"}}"#;
        let out = sanitize_recording(raw, &["p"], &[]).unwrap();
        assert!(!out[0].1.contains("alice"));
        let l = lines(&out[0].1);
        let cwd: Vec<_> = l.iter().map(|x| x["payload"]["cwd"].clone()).collect();
        assert_eq!(
            cwd,
            [
                "C:\\fixture\\home",
                "C:\\fixture\\project",
                "C:\\fixture\\home",
                "C:\\fixture\\home",
                "C:\\fixture\\demo",
                "C:\\fixture\\project",
            ]
        );
        assert_eq!(l[0]["payload"]["tool_name"], "other");
        assert_eq!(l[1]["payload"]["error"], "other");
        assert_eq!(project_name(r"C:\Users\alice", &[]), "home");
    }

    #[test]
    fn home_folder_is_home_by_user_name_or_place() {
        let alice = ["alice".to_string()];
        for (cwd, users, want) in [
            (r"D:\Profiles\alice", &alice[..], "home"),
            (r"D:\Profiles\ALICE\", &alice[..], "home"),
            ("/home/alice", &alice[..], "home"),
            (r"C:\Users\.\alice", &[], "home"),
            (r"C:\Users\alice\..\bob", &[], "home"),
            (r"C:\Users\alice\code\demo", &alice[..], "demo"),
            (r"C:\Users\alice\code\.", &[], "code"),
            (r"D:\Profiles\alice", &[], "alice"),
        ] {
            assert_eq!(project_name(cwd, users), want, "{cwd}");
        }
    }

    #[test]
    fn errors_name_the_real_file_line() {
        let raw = "\n\n{\"tMs\":1,\"hook\":\"Stop\",\"payload\":{\"session_id\":\"x\"}}\n\n{\"tMs\":2,\"hook\":\"Stop\",\"payload\":{}}";
        let e = sanitize_recording(raw, &["x"], &[]).unwrap_err();
        assert_eq!(e, "recording line 5: no session_id");
    }

    const OK_EXPECTED: &str = r#"{"timeline":[{"tMs":0,"sessionId":"s1","state":"needs_input","tier":"needs_you"},{"tMs":9,"sessionId":"s2","state":"ended_unexpectedly","tier":"resting"}],"effects":[{"tMs":0,"type":"Notify","sessionId":"s1","reason":"needs_permission"},{"tMs":5,"type":"Escalate","sessionId":"s1"},{"tMs":6,"type":"ReadyBatchAdd","sessionId":"s2"},{"tMs":7,"type":"Persist"},{"tMs":8,"type":"Persist","sessionId":"s2"}]}"#;

    #[test]
    fn check_expected_rejects_unsanitized_files() {
        check_expected(OK_EXPECTED).unwrap();
        check_expected(r#"{"effects":[],"timeline":[]}"#).unwrap();
        let tl = |e: &str| format!(r#"{{"timeline":[{e}],"effects":[]}}"#);
        let fx = |e: &str| format!(r#"{{"timeline":[],"effects":[{e}]}}"#);
        let uuid = "0b1f6c2e-3d4a-4b5c-8d9e-112233445566";
        for bad in [
            // The reviewer's example: an extra real-id key and a UUID as state.
            tl(&format!(
                r#"{{"tMs":0,"sessionId":"s1","session_id":"{uuid}","state":"{uuid}","tier":"working"}}"#
            )),
            tl(&format!(
                r#"{{"tMs":0,"sessionId":"s1","session_id":"{uuid}","state":"working","tier":"working"}}"#
            )),
            tl(&format!(
                r#"{{"tMs":0,"sessionId":"s1","state":"{uuid}","tier":"working"}}"#
            )),
            tl(
                r#"{"tMs":0,"sessionId":"s1","state":"working","tier":"working","message":"send-the-key-to-bob"}"#,
            ),
            fx(
                r#"{"tMs":0,"type":"Notify","sessionId":"s1","reason":"stalled","message":"send-the-key-to-bob"}"#,
            ),
            r#"{"timeline":[],"effects":[],"message":"send-the-key-to-bob"}"#.into(),
            r#"{"timeline":[]}"#.into(),
            r#"{"timeline":{},"effects":[]}"#.into(),
            tl(r#"{"tMs":-1,"sessionId":"s1","state":"working","tier":"working"}"#),
            tl(r#"{"tMs":1.5,"sessionId":"s1","state":"working","tier":"working"}"#),
            tl(r#"{"tMs":0,"sessionId":"a1","state":"working","tier":"working"}"#),
            tl(r#"{"tMs":0,"state":"working","tier":"working"}"#),
            tl(r#"{"tMs":0,"sessionId":"s1","state":"working","tier":"Working"}"#),
            tl(r#"{"tMs":0,"sessionId":"s1","state":"needs_you","tier":"needs_you"}"#),
            tl("1"),
            fx(r#"{"tMs":0,"type":"Notify","sessionId":"s1"}"#),
            fx(r#"{"tMs":0,"type":"Notify","sessionId":"s1","reason":"other"}"#),
            fx(r#"{"tMs":0,"type":"Escalate","sessionId":"s1","reason":"stalled"}"#),
            fx(r#"{"tMs":0,"type":"Escalate"}"#),
            fx(r#"{"tMs":0,"type":"ReadyBatchAdd"}"#),
            fx(r#"{"tMs":0,"type":"Toast","sessionId":"s1"}"#),
            fx(r#"{"tMs":0,"type":"Persist","sessionId":"real-id"}"#),
            fx(r#"{"type":"Persist"}"#),
        ] {
            assert!(check_expected(&bad).is_err(), "accepted {bad}");
        }
        for bad in [
            "not json",
            r#"{"timeline":[{"sessionId":"0b1f-real"}]}"#,
            r#"{"timeline":[{"sessionId":7}]}"#,
            r#"{"timeline":[{"sessionId":"a1"}]}"#,
            r#"{"timeline":[{"cwd":"C:\\Users\\alice"}]}"#,
            r#"{"timeline":[{"x":"appdata"}]}"#,
            r#"{"timeline":[{"x":"some free text"}]}"#,
            r#"{"timeline":[{"some free text":1}]}"#,
            r#"["D:"]"#,
        ] {
            assert!(check_expected(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn check_walks_the_folder_and_fails_on_any_other_file() {
        let dir = std::env::temp_dir().join(format!("knock-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        let ok = r#"{"tMs":0,"hook":"Stop","payload":{"session_id":"s1","cwd":"C:\\fixture\\p","hook_event_name":"Stop"}}"#;
        fs::write(dir.join("a.jsonl"), ok).unwrap();
        fs::write(dir.join("a.expected.json"), OK_EXPECTED).unwrap();
        fs::write(dir.join("sub").join("b.jsonl"), ok).unwrap();
        assert_eq!(check(&dir), Ok(3));
        let unsanitized = ok.replace("s1", "real-id");
        let real_sid = OK_EXPECTED.replace("s1", "real-id");
        let path = OK_EXPECTED.replace("Notify", "C:\\\\Users\\\\a");
        for (file, body) in [
            ("x.JSONL", ok),
            ("x.Expected.Json", OK_EXPECTED),
            ("x.json", OK_EXPECTED),
            ("notes.txt", ""),
            ("noext", ""),
            ("sub/deep.txt", ""),
            ("sub/c.jsonl", &unsanitized),
            ("c.expected.json", "not json"),
            ("c.expected.json", &real_sid),
            ("c.expected.json", &path),
        ] {
            let p = dir.join(file);
            fs::write(&p, body).unwrap();
            let r = check(&dir);
            fs::remove_file(&p).unwrap();
            assert!(r.is_err(), "accepted {file}: {body}");
        }
        assert_eq!(check(&dir), Ok(3));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Guards the repo: every file in `fixtures/` must be sanitized (§8.8, §11.3).
    #[test]
    fn committed_fixtures_are_sanitized() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let n = check(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert!(n > 0, "no fixtures found");
    }
}
