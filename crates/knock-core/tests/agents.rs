//! Other agents' hooks (ADR 0039): Codex CLI and GitHub Copilot CLI send Claude
//! Code's payloads, so they share its parser. Payload shapes follow their
//! hook documentation; free text (prompts, tool input) never gets through.

use knock_core::{
    AGENTS, Adapter, Agy, Category, ClaudeCode, Codex, Copilot, Kind, ParseError, adapter,
};
use serde_json::json;

const T: u64 = 1_000;

fn parse(a: &dyn Adapter, v: serde_json::Value) -> Result<Option<knock_core::Event>, ParseError> {
    a.parse(v.to_string().as_bytes(), T)
}

fn codex(event: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({
        "session_id": "0198-aaaa", "transcript_path": null, "cwd": "/home/a/work/api",
        "hook_event_name": event, "model": "gpt-5", "turn_id": "t1", "permission_mode": "default",
        "prompt": "SECRET prompt text", "tool_input": {"command": "cat ~/.ssh/id_rsa"},
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[test]
fn codex_events_map_like_claude_codes() {
    let a = Codex {
        users: vec!["a".into()],
    };
    let kind = |e: &str, extra| parse(&a, codex(e, extra)).unwrap().map(|x| x.kind);
    assert_eq!(kind("UserPromptSubmit", json!({})), Some(Kind::TurnStarted));
    assert_eq!(
        kind("PostToolUse", json!({"tool_name": "Bash"})),
        Some(Kind::Activity)
    );
    assert_eq!(
        kind("PermissionRequest", json!({"tool_name": "Bash"})),
        Some(Kind::NeedsPermission)
    );
    assert_eq!(kind("Stop", json!({})), Some(Kind::TurnCompleted));
    assert_eq!(kind("Interrupt", json!({})), Some(Kind::TurnIdle));
    assert_eq!(kind("SessionEnd", json!({})), Some(Kind::SessionEnded));
    // Events Knock doesn't use are ignored; ones it doesn't register (PreToolUse,
    // which could block the agent) would still read as plain activity.
    for e in ["SessionStart", "Bogus"] {
        assert_eq!(kind(e, json!({"tool_name": "Bash"})), None, "{e}");
    }
    assert_eq!(
        kind("PreToolUse", json!({"tool_name": "Bash"})),
        Some(Kind::Activity)
    );
    let registered: Vec<_> = a.hooks().events.iter().map(|(e, _)| *e).collect();
    assert_eq!(
        registered,
        [
            "UserPromptSubmit",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
            "Interrupt",
            "SessionEnd"
        ]
    );
    let e = parse(&a, codex("PermissionRequest", json!({"tool_name": "Bash"})))
        .unwrap()
        .unwrap();
    assert_eq!(
        (e.agent, e.session_id.as_str(), e.context.project.as_str()),
        ("codex", "codex-0198-aaaa", "api")
    );
    assert_eq!(e.detail.tool.as_deref(), Some("Bash"));
    let e = parse(
        &a,
        codex("PostToolUse", json!({"tool_name": "apply_patch"})),
    )
    .unwrap()
    .unwrap();
    assert_eq!(e.detail.category, Some(Category::of("apply_patch")));
}

#[test]
fn free_text_never_gets_through() {
    for a in [
        &Codex { users: vec![] } as &dyn Adapter,
        &Copilot { users: vec![] },
    ] {
        let e = parse(
            a,
            codex(
                "PermissionRequest",
                json!({"tool_name": "Bash", "message": "SECRET"}),
            ),
        )
        .unwrap()
        .unwrap();
        let text = serde_json::to_string(&e).unwrap();
        for secret in ["SECRET", "id_rsa", "transcript", "gpt-5"] {
            assert!(!text.contains(secret), "{} leaked {secret}: {text}", a.id());
        }
    }
}

#[test]
fn copilot_notifications_are_the_permission_and_question_signals() {
    let a = Copilot { users: vec![] };
    let note = |t: &str| {
        let v = json!({"session_id": "c-1", "cwd": "/w/repo", "hook_event_name": "Notification",
                       "notification_type": t, "title": "T", "message": "SECRET"});
        parse(&a, v).unwrap().map(|e| e.kind)
    };
    assert_eq!(note("permission_prompt"), Some(Kind::NeedsPermission));
    assert_eq!(note("elicitation_dialog"), Some(Kind::NeedsAnswer));
    // A background agent finishing a turn isn't the user's session waiting.
    assert_eq!(note("agent_idle"), None);
    assert_eq!(note("shell_completed"), None);
    let stop = parse(
        &a,
        json!({"session_id": "c-1", "cwd": "/w/repo", "hook_event_name": "Stop"}),
    );
    let e = stop.unwrap().unwrap();
    assert_eq!(
        (e.kind, e.agent, e.session_id.as_str()),
        (Kind::TurnCompleted, "copilot", "copilot-c-1")
    );
    // Copilot's `PermissionRequest` runs even when nothing asks the user, and
    // Knock doesn't register it; if one arrives anyway it still counts only
    // when the agent names a tool, as for Claude Code.
    assert_eq!(
        a.hooks()
            .events
            .iter()
            .filter(|(e, _)| *e == "PermissionRequest")
            .count(),
        0
    );
    assert_eq!(
        a.hooks()
            .events
            .iter()
            .filter(|(e, _)| *e == "PreToolUse")
            .count(),
        0
    );
}

/// Copilot's hook files name events in camelCase and its payloads then have no event
/// name (the helper adds the registered one): `sessionId`, `toolName`, camelCase.
#[test]
fn copilot_reads_camel_case_events_and_payloads() {
    let a = Copilot { users: vec![] };
    let camel = |event: &str, extra: serde_json::Value| {
        let mut v = json!({"hook_event_name": event, "sessionId": "c-9", "timestamp": 1,
                           "cwd": "/w/repo", "prompt": "SECRET"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        parse(&a, v).unwrap()
    };
    let kind = |e: &str, x| camel(e, x).map(|e| e.kind);
    assert_eq!(
        kind("userPromptSubmitted", json!({})),
        Some(Kind::TurnStarted)
    );
    assert_eq!(
        kind("postToolUse", json!({"toolName": "bash"})),
        Some(Kind::Activity)
    );
    assert_eq!(
        kind(
            "notification",
            json!({"notification_type": "permission_prompt"})
        ),
        Some(Kind::NeedsPermission)
    );
    assert_eq!(
        kind(
            "notification",
            json!({"notificationType": "elicitation_dialog"})
        ),
        Some(Kind::NeedsAnswer)
    );
    assert_eq!(
        kind("notification", json!({"notification_type": "agent_idle"})),
        None
    );
    assert_eq!(kind("agentStop", json!({})), Some(Kind::TurnCompleted));
    assert_eq!(
        kind("sessionEnd", json!({"reason": "complete"})),
        Some(Kind::SessionEnded)
    );
    for unused in ["sessionStart", "preToolUse", "errorOccurred"] {
        assert_eq!(kind(unused, json!({})), None, "{unused}");
    }
    let e = camel("postToolUse", json!({"toolName": "bash"})).unwrap();
    assert_eq!(
        (e.session_id.as_str(), e.detail.tool.as_deref()),
        ("copilot-c-9", Some("bash"))
    );
    // The registered names are exactly the camelCase ones Copilot documents.
    let registered: Vec<_> = a.hooks().events.iter().map(|(e, _)| *e).collect();
    assert_eq!(
        registered,
        [
            "userPromptSubmitted",
            "postToolUse",
            "notification",
            "agentStop",
            "sessionEnd"
        ]
    );
}

#[test]
fn sessions_of_different_agents_never_share_an_id() {
    let same = json!({"session_id": "abc", "cwd": "/w/p", "hook_event_name": "UserPromptSubmit"});
    let ids: Vec<String> = [
        &ClaudeCode { users: vec![] } as &dyn Adapter,
        &Codex { users: vec![] },
        &Copilot { users: vec![] },
    ]
    .iter()
    .map(|a| parse(*a, same.clone()).unwrap().unwrap().session_id)
    .collect();
    assert_eq!(ids, ["abc", "codex-abc", "copilot-abc"]);
    let agy = parse(
        &Agy { users: vec![] },
        json!({"hook_event_name": "Stop", "conversationId": "abc", "workspacePaths": ["/w/p"]}),
    );
    assert_eq!(agy.unwrap().unwrap().session_id, "agy-abc");
    // Bad ids are refused, and a prefix can't push a long one past the limit.
    let a = Codex { users: vec![] };
    for bad in ["a/b", "a b", "", &"x".repeat(125)] {
        let v = json!({"session_id": bad, "cwd": "/w", "hook_event_name": "Stop"});
        assert!(parse(&a, v).is_err(), "{bad:?}");
    }
}

#[test]
fn every_agent_has_an_adapter_with_its_own_path() {
    let mut paths = Vec::new();
    for id in AGENTS {
        let a = adapter(id, vec![]).expect(id);
        assert_eq!(a.id(), id);
        assert!(!a.display_name().is_empty());
        assert_eq!(a.hooks().path, format!("/v1/hooks/{id}"));
        paths.push(a.hooks().path);
    }
    paths.dedup();
    assert_eq!(paths.len(), AGENTS.len());
    assert!(adapter("gemini", vec![]).is_none());
}

/// A payload as `agy` sends it (shapes from real captures: antigravity's
/// docs/agy-hook-samples), plus the `hook_event_name` that `knock-desktop hook agy
/// <Event>` adds, since agy's own payload has no event name.
fn agy(event: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({
        "hook_event_name": event,
        "conversationId": "60644c6c-00fd-42a2-9bcc-ce8c58d4c2c5",
        "workspacePaths": ["/home/a/work/api"],
        "lastUserInput": "SECRET prompt text",
        "modelName": "gemini-4-argon-high",
        "transcriptPath": "/home/a/.gemini/antigravity-cli/brain/x/transcript_full.jsonl",
        "artifactDirectoryPath": "/home/a/.gemini/antigravity-cli/brain/x",
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[test]
fn agy_events_map_to_a_turn_and_its_activity() {
    let a = Agy {
        users: vec!["a".into()],
    };
    let kind = |e: &str, extra| parse(&a, agy(e, extra)).unwrap().map(|x| x.kind);
    let read = json!({"stepIdx": 2, "toolCall": {"name": "read_file", "args": {"Path": "SECRET"}}});
    assert_eq!(kind("SessionStart", json!({})), Some(Kind::TurnStarted));
    assert_eq!(
        kind("PreInvocation", json!({"invocationNum": 0})),
        Some(Kind::TurnStarted)
    );
    assert_eq!(
        kind("PreInvocation", json!({"invocationNum": 1})),
        Some(Kind::Activity)
    );
    assert_eq!(kind("PreToolUse", read.clone()), Some(Kind::Activity));
    assert_eq!(kind("PostToolUse", read.clone()), Some(Kind::Activity));
    assert_eq!(
        kind("PostInvocation", json!({"invocationNum": 0})),
        Some(Kind::Activity)
    );
    assert_eq!(kind("Bogus", json!({})), None);
    let e = parse(&a, agy("PreToolUse", read)).unwrap().unwrap();
    assert_eq!(
        (e.agent, e.session_id.as_str()),
        ("agy", "agy-60644c6c-00fd-42a2-9bcc-ce8c58d4c2c5")
    );
    assert_eq!(
        (e.context.project.as_str(), e.detail.category),
        ("api", Some(Category::Read))
    );
    assert_eq!(e.detail.tool.as_deref(), Some("read_file"));
}

#[test]
fn agy_stop_says_how_the_turn_ended() {
    let a = Agy { users: vec![] };
    let stop = |reason: &str, idle: bool| {
        let v = agy(
            "Stop",
            json!({"executionNum": 0, "terminationReason": reason, "fullyIdle": idle, "error": ""}),
        );
        parse(&a, v).unwrap().unwrap()
    };
    assert_eq!(stop("NO_TOOL_CALL", true).kind, Kind::TurnCompleted);
    assert_eq!(stop("MAX_INVOCATIONS", true).kind, Kind::TurnCompleted);
    assert_eq!(stop("USER_CANCELED", true).kind, Kind::TurnIdle);
    let failed = stop("ERROR", true);
    assert_eq!(
        (failed.kind, failed.detail.reason.as_deref()),
        (Kind::AgentFailed, Some("error"))
    );
    // Background tasks still running: the agent isn't done.
    assert_eq!(stop("NO_TOOL_CALL", false).kind, Kind::Activity);
}

#[test]
fn agy_only_guesses_a_permission_prompt_from_an_unsandboxed_command() {
    let a = Agy { users: vec![] };
    let cmd = |name: &str, bypass: serde_json::Value| {
        let args = json!({"CommandLine": "SECRET", "BypassSandbox": bypass});
        let v = agy(
            "PreToolUse",
            json!({"stepIdx": 3, "toolCall": {"name": name, "args": args}}),
        );
        parse(&a, v).unwrap().unwrap()
    };
    let asks = cmd("run_command", json!(true));
    assert_eq!(
        (asks.kind, asks.detail.tool.as_deref()),
        (Kind::NeedsPermission, Some("run_command"))
    );
    assert_eq!(
        cmd("run_command", json!(false)).kind,
        Kind::Activity,
        "sandboxed commands just run"
    );
    assert_eq!(
        cmd("run_command", json!("yes")).kind,
        Kind::Activity,
        "only a real true counts"
    );
    assert_eq!(
        cmd("write_file", json!(true)).kind,
        Kind::Activity,
        "only commands are guessed"
    );
}

#[test]
fn agy_subagents_and_bad_payloads_are_not_sessions() {
    let a = Agy { users: vec![] };
    let sub = agy(
        "PostToolUse",
        json!({"parentConversationId": "root-1", "toolCall": {"name": "read_file"}}),
    );
    assert_eq!(parse(&a, sub).unwrap(), None);
    let mut none = agy("Stop", json!({}));
    none.as_object_mut().unwrap().remove("conversationId");
    assert_eq!(parse(&a, none), Err(ParseError::NoSessionId));
    let bad = agy("Stop", json!({"conversationId": "a/b"}));
    assert_eq!(parse(&a, bad), Err(ParseError::BadSessionId));
    assert!(a.parse(b"not json", 1).is_err());
    assert!(a.parse(b"[]", 1).is_err());
    // A wrongly typed field is ignored; the event still reads.
    let odd = agy(
        "Stop",
        json!({"fullyIdle": "yes", "invocationNum": "x", "workspacePaths": "nope", "toolCall": 7}),
    );
    let e = parse(&a, odd).unwrap().unwrap();
    assert_eq!(
        (e.kind, e.context.project.as_str()),
        (Kind::TurnCompleted, "")
    );
    // The agent's own text and paths never get through.
    let e = parse(
        &a,
        agy(
            "PreToolUse",
            json!({"toolCall": {"name": "read_file", "args": {"Path": "SECRET"}}}),
        ),
    )
    .unwrap()
    .unwrap();
    let text = serde_json::to_string(&e).unwrap();
    for leak in ["SECRET", "transcript", "argon", "brain"] {
        assert!(!text.contains(leak), "leaked {leak}: {text}");
    }
}
