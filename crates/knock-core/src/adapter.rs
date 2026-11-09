//! Agent adapters (SPEC.md §7.3) and the one Stage A adapter, `claude-code`
//! (§8.3 mapping; ADR 0007, 0009, 0010).

use crate::event::{Category, Context, Detail, Event, Kind};
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

/// What every agent integration provides (§7.3).
pub trait Adapter {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> &'static [Capability];
    /// Data only. Install, uninstall and the health check are built by the
    /// app from this (§8.2, FR-12; Phase 2).
    fn hooks(&self) -> HookSpec;
    /// Raw hook body → at most one normalized event. `ts` is the server
    /// receive time in ms since the Unix epoch. `Ok(None)` = ignored by design.
    fn parse(&self, body: &[u8], ts: u64) -> Result<Option<Event>, ParseError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    NeedsPermission,
    NeedsAnswer,
    Failed,
    Idle,
    BackgroundAgents,
}

/// The hooks an adapter registers: the server path that marks Knock's
/// entries (§8.2 ownership) and each event with its matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSpec {
    pub path: &'static str,
    pub events: &'static [(&'static str, Option<&'static str>)],
}

/// Carries no payload content, so it is safe to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Not JSON, not an object, or `session_id` or `hook_event_name` is not
    /// a string.
    BadPayload,
    NoSessionId,
    /// `session_id` is not `^[A-Za-z0-9_-]{1,128}$`.
    BadSessionId,
}

pub struct ClaudeCode {
    /// User names for the `home` rule; the app reads them (see [`project_with`]).
    pub users: Vec<String>,
}

/// The ADR 0009 allow-list. serde skips every other field without keeping
/// it, so free text never leaves the parser. Only `session_id` and
/// `hook_event_name` must have the right type; any other field that is not
/// a string counts as missing.
#[derive(Deserialize)]
struct Raw {
    session_id: Option<String>,
    hook_event_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    cwd: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    tool_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    notification_type: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    error: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    agent_id: Option<String>,
}

/// A string, or `None` for any other value, which is skipped unkept.
fn string_or_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("any value")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.into()))
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
            IgnoredAny.visit_seq(a).map(|_| None)
        }
        fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
            IgnoredAny.visit_map(a).map(|_| None)
        }
    }
    d.deserialize_any(V)
}

fn charset(s: &str, max: usize, ok: impl Fn(char) -> bool) -> bool {
    (1..=max).contains(&s.len()) && s.chars().all(ok)
}

/// `^[A-Za-z0-9_-]{1,128}$`, for `session_id` and `agent_id`.
fn is_id(s: &str) -> bool {
    charset(s, 128, |c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    })
}

impl Adapter for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }
    fn display_name(&self) -> &'static str {
        "Claude Code"
    }
    fn capabilities(&self) -> &'static [Capability] {
        &[
            Capability::NeedsPermission,
            Capability::NeedsAnswer,
            Capability::Failed,
            Capability::Idle,
            Capability::BackgroundAgents,
        ]
    }
    fn hooks(&self) -> HookSpec {
        HookSpec {
            path: "/v1/hooks/claude-code",
            events: &[
                ("UserPromptSubmit", None),
                ("PreToolUse", Some("*")),
                ("PostToolUse", Some("*")),
                ("PermissionRequest", Some("*")),
                ("Notification", None),
                ("Stop", None),
                ("StopFailure", None),
                ("SubagentStart", None),
                ("SubagentStop", None),
                ("PreCompact", None),
                ("SessionEnd", None),
            ],
        }
    }

    fn parse(&self, body: &[u8], ts: u64) -> Result<Option<Event>, ParseError> {
        let raw: Raw = serde_json::from_slice(body).map_err(|_| ParseError::BadPayload)?;
        let session_id = raw
            .session_id
            .filter(|s| !s.is_empty())
            .ok_or(ParseError::NoSessionId)?;
        if !is_id(&session_id) {
            return Err(ParseError::BadSessionId);
        }
        // Shown in chips and notifications: a name, never anything longer.
        let tool = raw.tool_name.map(|t| {
            if charset(&t, 128, |c| c.is_ascii_alphanumeric() || "_.:-".contains(c)) {
                t
            } else {
                "other".into()
            }
        });
        let only = |tool: Option<String>| Detail {
            tool,
            ..Detail::default()
        };
        let (kind, detail) = match raw.hook_event_name.as_deref().unwrap_or("") {
            "UserPromptSubmit" => (Kind::TurnStarted, Detail::default()),
            "PreToolUse" | "PermissionRequest" if tool.as_deref() == Some("AskUserQuestion") => {
                (Kind::NeedsAnswer, Detail::default())
            }
            "PreToolUse" | "PostToolUse" => (
                Kind::Activity,
                Detail {
                    category: tool.as_deref().map(Category::of),
                    tool,
                    ..Detail::default()
                },
            ),
            // Always a tool here: the engine tells a PermissionRequest from a
            // `permission_prompt` Notification (no tool) by it (§8.4 dedup).
            "PermissionRequest" => (
                Kind::NeedsPermission,
                only(Some(tool.unwrap_or_else(|| "other".into()))),
            ),
            "Notification" => match raw.notification_type.as_deref().unwrap_or("") {
                "permission_prompt" => (Kind::NeedsPermission, Detail::default()),
                "elicitation_dialog" => (Kind::NeedsAnswer, Detail::default()),
                "idle_prompt" => (Kind::TurnIdle, Detail::default()),
                _ => return Ok(None),
            },
            "Stop" => (Kind::TurnCompleted, Detail::default()),
            "StopFailure" => {
                let code = raw.error.filter(|e| {
                    charset(e, 64, |c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
                    })
                });
                (
                    Kind::AgentFailed,
                    Detail {
                        reason: Some(code.unwrap_or_else(|| "unknown".into())),
                        ..Detail::default()
                    },
                )
            }
            "SubagentStart" | "SubagentStop" | "PreCompact" => (Kind::Activity, Detail::default()),
            "SessionEnd" => (Kind::SessionEnded, Detail::default()),
            _ => return Ok(None),
        };
        let cwd = raw.cwd.unwrap_or_default();
        Ok(Some(Event {
            v: 1,
            agent: self.id(),
            session_id,
            ts,
            kind,
            detail,
            context: Context {
                project: project_with(&cwd, &self.users).into(),
                cwd,
                pid: None,
                // Never free text; a bad one is still one requester (ADR 0019).
                agent_id: raw
                    .agent_id
                    .map(|a| if is_id(&a) { a } else { "other".into() }),
            },
        }))
    }
}

/// `project` (§8.3): the last part of `cwd`, with `.` dropped and `..`
/// applied, or `home` for a home folder (directly under `Users` or, on
/// Linux, `home`, or named like a `users` entry), so a user name is never
/// shown as a project.
/// Same rule as `apps/desktop/src-tauri/src/jump.rs`.
pub fn project_with<'a>(cwd: &'a str, users: &[String]) -> &'a str {
    let mut parts = Vec::new();
    for p in cwd.split(['\\', '/']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    let last = parts.last().copied().unwrap_or("");
    let parent = parts.len().checked_sub(2).map_or("", |i| parts[i]);
    let lower = last.to_lowercase();
    let home_parent = parent.eq_ignore_ascii_case("users") || parent == "home";
    if home_parent || users.iter().any(|u| u.to_lowercase() == lower) {
        "home"
    } else {
        last
    }
}

/// The `animalOverrides` project key (§8.6): 64-bit FNV-1a of the UTF-8 bytes
/// of `cwd.to_lowercase()`, as 16 lower-case hex digits. Stable across
/// versions; changing it orphans users' overrides.
pub fn project_key(cwd: &str) -> String {
    let h = cwd
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("{h:016x}")
}
