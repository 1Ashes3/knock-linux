//! §12.1 Adapter: every fixture payload → normalized events (insta), the §8.3
//! mapping, and free text dropped at parse time (§11.3, ADR 0009).

use knock_core::{
    Adapter, Capability, Category, ClaudeCode, Kind, ParseError, iso8601, project_key, project_with,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const BASE: u64 = 1_792_159_331_120; // 2026-10-16T14:02:11.120Z, the §8.3 example

fn cc() -> ClaudeCode {
    ClaudeCode { users: Vec::new() }
}

fn parse(v: Value) -> Result<Option<knock_core::Event>, ParseError> {
    cc().parse(v.to_string().as_bytes(), BASE)
}

#[test]
fn every_fixture_payload_snapshots() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".jsonl").map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names.len(), 17);
    for name in names {
        let text = fs::read_to_string(dir.join(format!("{name}.jsonl"))).unwrap();
        let events: Vec<Value> = text
            .lines()
            .map(|l| {
                let line: Value = serde_json::from_str(l).unwrap();
                let t = line["tMs"].as_u64().unwrap();
                let body = line["payload"].to_string();
                let e = cc().parse(body.as_bytes(), BASE + t).unwrap();
                json!({ "tMs": t, "hook": line["hook"], "event": e })
            })
            .collect();
        insta::assert_json_snapshot!(name, events);
    }
}

#[test]
fn free_text_never_leaves_the_parser() {
    let secret = "SECRET do not keep";
    let free = json!({
        "prompt": secret, "message": secret, "title": secret,
        "tool_input": { "command": secret, "file_path": r"C:\Users\someone\x.txt" },
        "tool_response": { "stdout": secret },
        "last_assistant_message": secret, "custom_instructions": secret,
        "transcript_path": r"C:\Users\someone\t.jsonl",
        "agent_transcript_path": r"C:\Users\someone\a.jsonl",
        "scratchpad_dir": r"C:\Users\someone\scratch",
        "reason": secret, "trigger": secret, "agent_type": secret,
        "permission_mode": "default", "effort": { "level": secret },
        "prompt_id": secret, "tool_use_id": secret, "duration_ms": 5,
        "stop_hook_active": false, "background_tasks": [secret], "session_crons": [secret],
    });
    for (hook, extra) in [
        ("UserPromptSubmit", json!({})),
        ("PreToolUse", json!({ "tool_name": "Bash" })),
        ("PreToolUse", json!({ "tool_name": "AskUserQuestion" })),
        ("PostToolUse", json!({ "tool_name": "Write" })),
        ("PermissionRequest", json!({ "tool_name": "PowerShell" })),
        ("PermissionRequest", json!({})),
        (
            "Notification",
            json!({ "notification_type": "permission_prompt" }),
        ),
        (
            "Notification",
            json!({ "notification_type": "elicitation_dialog" }),
        ),
        (
            "Notification",
            json!({ "notification_type": "idle_prompt" }),
        ),
        ("Stop", json!({})),
        ("StopFailure", json!({ "error": "rate_limit" })),
        ("StopFailure", json!({ "error": "SECRET do not keep" })),
        ("SubagentStart", json!({ "agent_id": "a1" })),
        ("SubagentStop", json!({ "agent_id": "a1" })),
        ("PreCompact", json!({})),
        ("SessionEnd", json!({})),
        // Free text inside allow-listed fields becomes a placeholder.
        ("PreToolUse", json!({ "tool_name": "SECRET do not keep" })),
        (
            "PermissionRequest",
            json!({ "tool_name": r"C:\Users\someone" }),
        ),
    ] {
        let mut p = free.clone();
        let o = p.as_object_mut().unwrap();
        o.extend(extra.as_object().unwrap().clone());
        o.insert("hook_event_name".into(), hook.into());
        o.insert("session_id".into(), "s1".into());
        o.insert("cwd".into(), r"D:\work\demo".into());
        let e = parse(p)
            .unwrap()
            .unwrap_or_else(|| panic!("{hook} ignored"));
        let out = format!("{} {e:?}", serde_json::to_string(&e).unwrap());
        for bad in ["SECRET", "Users", "someone", "default", "prompt_id"] {
            assert!(!out.contains(bad), "{hook} kept {bad}");
        }
    }
}

#[test]
fn mapping_follows_section_8_3() {
    let p = |hook: &str, extra: Value| {
        let mut v =
            json!({ "session_id": "s1", "cwd": r"C:\fixture\demo", "hook_event_name": hook });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        parse(v).unwrap()
    };
    let kind = |hook, extra| p(hook, extra).map(|e| e.kind);
    assert_eq!(kind("UserPromptSubmit", json!({})), Some(Kind::TurnStarted));
    for hook in ["PreToolUse", "PermissionRequest"] {
        let e = p(hook, json!({ "tool_name": "AskUserQuestion" })).unwrap();
        assert_eq!((e.kind, e.detail.tool), (Kind::NeedsAnswer, None), "{hook}");
    }
    let e = p("PostToolUse", json!({ "tool_name": "AskUserQuestion" })).unwrap();
    assert_eq!(
        (e.kind, e.detail.category),
        (Kind::Activity, Some(Category::Other))
    );
    let e = p(
        "PermissionRequest",
        json!({ "tool_name": "PowerShell", "agent_id": "a1" }),
    )
    .unwrap();
    assert_eq!(e.kind, Kind::NeedsPermission);
    assert_eq!(e.detail.tool.as_deref(), Some("PowerShell"));
    assert_eq!(e.context.agent_id.as_deref(), Some("a1"));
    let e = p("PermissionRequest", json!({})).unwrap();
    assert_eq!(
        e.detail.tool.as_deref(),
        Some("other"),
        "a request always has a tool"
    );
    let e = p(
        "Notification",
        json!({ "notification_type": "permission_prompt" }),
    )
    .unwrap();
    assert_eq!((e.kind, e.detail.tool), (Kind::NeedsPermission, None));
    let n = |t: &str| kind("Notification", json!({ "notification_type": t }));
    assert_eq!(n("elicitation_dialog"), Some(Kind::NeedsAnswer));
    assert_eq!(n("idle_prompt"), Some(Kind::TurnIdle));
    for other in [
        "auth_success",
        "agent_needs_input",
        "elicitation_url_dialog",
        "",
    ] {
        assert_eq!(n(other), None, "{other}");
    }
    assert_eq!(kind("Notification", json!({})), None);
    assert_eq!(kind("Stop", json!({})), Some(Kind::TurnCompleted));
    let reason = |extra| p("StopFailure", extra).unwrap().detail.reason;
    assert_eq!(
        reason(json!({ "error": "model_not_found" })).as_deref(),
        Some("model_not_found")
    );
    assert_eq!(reason(json!({})).as_deref(), Some("unknown"));
    assert_eq!(
        reason(json!({ "error": "Rate Limit" })).as_deref(),
        Some("unknown")
    );
    for hook in ["SubagentStart", "SubagentStop", "PreCompact"] {
        let e = p(hook, json!({})).unwrap();
        assert_eq!(
            (e.kind, e.detail.tool, e.detail.category),
            (Kind::Activity, None, None)
        );
    }
    assert_eq!(
        kind("SessionEnd", json!({ "reason": "other" })),
        Some(Kind::SessionEnded)
    );
    // SessionStart has no HTTP producer (ADR 0005); unknown hooks are ignored.
    for hook in [
        "SessionStart",
        "PostToolUseFailure",
        "PermissionDenied",
        "Nope",
        "",
    ] {
        assert_eq!(kind(hook, json!({})), None, "{hook}");
    }
    let e = p("Stop", json!({})).unwrap();
    assert_eq!((e.v, e.agent, e.ts), (1, "claude-code", BASE));
    assert_eq!(
        (
            e.context.cwd.as_str(),
            e.context.project.as_str(),
            e.context.pid
        ),
        (r"C:\fixture\demo", "demo", None)
    );
}

#[test]
fn tool_categories() {
    for (tools, cat) in [
        (&["Read", "NotebookRead"][..], Category::Read),
        (&["Grep", "Glob"], Category::Search),
        (
            &["Edit", "MultiEdit", "Write", "NotebookEdit"],
            Category::Edit,
        ),
        (&["Bash", "PowerShell"], Category::Run),
        (&["WebFetch", "WebSearch"], Category::Web),
        (
            &["Agent", "mcp__github__list", "AskUserQuestion", "read"],
            Category::Other,
        ),
    ] {
        for t in tools {
            assert_eq!(Category::of(t), cat, "{t}");
        }
    }
}

#[test]
fn bad_bodies_are_errors_without_content() {
    let bad = |b: &str| cc().parse(b.as_bytes(), 0).unwrap_err();
    assert_eq!(bad("not json"), ParseError::BadPayload);
    assert_eq!(bad("[1]"), ParseError::BadPayload);
    assert_eq!(
        bad(r#"{"session_id":7,"hook_event_name":"Stop"}"#),
        ParseError::BadPayload
    );
    assert_eq!(
        bad(r#"{"hook_event_name":"Stop"}"#),
        ParseError::NoSessionId
    );
    assert_eq!(
        bad(r#"{"session_id":"","hook_event_name":"Stop"}"#),
        ParseError::NoSessionId
    );
}

#[test]
fn project_is_the_last_cwd_part_and_home_hides_the_user() {
    let alice = ["alice".to_string()];
    for (cwd, users, want) in [
        (r"C:\code\knock", &[][..], "knock"),
        (r"C:\code\knock\", &[], "knock"),
        ("C:/code/knock", &[], "knock"),
        (r"C:\code\knock\.", &[], "knock"),
        (r"C:\code\knock\sub\..", &[], "knock"),
        (r"C:\Users\alice", &[], "home"),
        (r"D:\Profiles\alice", &alice[..], "home"),
        (r"D:\Profiles\ALICE\", &alice[..], "home"),
        (r"C:\Users\alice\..\bob", &[], "home"),
        (r"C:\Users\alice\code\demo", &alice[..], "demo"),
        (r"D:\Profiles\alice", &[], "alice"),
        ("", &[], ""),
    ] {
        assert_eq!(project_with(cwd, users), want, "{cwd}");
    }
    let mut cc = cc();
    cc.users = alice.to_vec();
    let body = br#"{"session_id":"s1","cwd":"D:\\Profiles\\Alice","hook_event_name":"Stop"}"#;
    assert_eq!(cc.parse(body, 0).unwrap().unwrap().context.project, "home");
}

#[test]
fn project_key_is_fixed_fnv1a_of_the_lower_cased_cwd() {
    assert_eq!(project_key(""), "cbf29ce484222325");
    assert_eq!(project_key("a"), "af63dc4c8601ec8c");
    assert_eq!(project_key("A"), project_key("a"));
    assert_eq!(project_key(r"C:\Code\Knock"), project_key(r"c:\code\knock"));
    assert_ne!(
        project_key(r"C:\code\knock"),
        project_key(r"C:\code\knock2")
    );
}

#[test]
fn adapter_identity_capabilities_and_hooks() {
    let cc = cc();
    assert_eq!((cc.id(), cc.display_name()), ("claude-code", "Claude Code"));
    assert!(cc.capabilities().contains(&Capability::NeedsPermission));
    let hooks = cc.hooks();
    assert_eq!(hooks.path, "/v1/hooks/claude-code");
    let names: Vec<_> = hooks.events.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Notification",
            "Stop",
            "StopFailure",
            "SubagentStart",
            "SubagentStop",
            "PreCompact",
            "SessionEnd",
        ]
    );
    for (n, m) in hooks.events {
        let tool = matches!(*n, "PreToolUse" | "PostToolUse" | "PermissionRequest");
        assert_eq!(*m, tool.then_some("*"), "{n}");
    }
}

#[test]
fn timestamps_are_iso_8601_utc() {
    assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso8601(BASE), "2026-10-16T14:02:11.120Z");
    assert_eq!(iso8601(951_868_799_999), "2000-02-29T23:59:59.999Z");
}
