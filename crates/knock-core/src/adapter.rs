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
    // Copilot's camelCase payloads (events registered as `agentStop` and so on)
    // use the same names in camelCase.
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    hook_event_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    cwd: Option<String>,
    #[serde(default, alias = "toolName", deserialize_with = "string_or_none")]
    tool_name: Option<String>,
    #[serde(
        default,
        alias = "notificationType",
        deserialize_with = "string_or_none"
    )]
    notification_type: Option<String>,
    #[serde(default, deserialize_with = "string_or_none")]
    error: Option<String>,
    #[serde(default, alias = "agentId", deserialize_with = "string_or_none")]
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

/// Codex CLI (ADR 0039): hooks with Claude Code's payloads and event names.
/// Sessions are named `codex-<session_id>`, so ids of different agents never meet.
pub struct Codex {
    /// User names for the `home` rule (see [`project_with`]).
    pub users: Vec<String>,
}

/// GitHub Copilot CLI (ADR 0039): hooks registered under PascalCase event
/// names send the VS Code / Claude-style payloads (`session_id`, `tool_name`,
/// `notification_type`). Sessions are named `copilot-<session_id>`.
pub struct Copilot {
    /// User names for the `home` rule (see [`project_with`]).
    pub users: Vec<String>,
}

/// Antigravity CLI, `agy` (ADR 0039). Its hooks differ from Claude Code's: camelCase
/// payloads with no event name (the helper adds `hook_event_name`, from its own
/// argument), `conversationId` and `workspacePaths` instead of `session_id` and `cwd`,
/// no permission-request event and no session-end event. Sessions are named
/// `agy-<conversationId>`.
pub struct Agy {
    /// User names for the `home` rule (see [`project_with`]).
    pub users: Vec<String>,
}

/// Every agent Knock reads hooks from, in the order Settings lists them.
pub const AGENTS: [&str; 4] = ["claude-code", "codex", "copilot", "agy"];

/// The adapter for agent `id` (one of [`AGENTS`]).
pub fn adapter(id: &str, users: Vec<String>) -> Option<Box<dyn Adapter + Send>> {
    match id {
        "claude-code" => Some(Box::new(ClaudeCode { users })),
        "codex" => Some(Box::new(Codex { users })),
        "copilot" => Some(Box::new(Copilot { users })),
        "agy" => Some(Box::new(Agy { users })),
        _ => None,
    }
}

/// How an agent's payloads differ from Claude Code's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Claude,
    Codex,
    Copilot,
    Agy,
}

impl Dialect {
    fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Agy => "agy",
        }
    }

    /// The event's name in Claude Code's terms. Copilot's hook files name events in
    /// camelCase (`agentStop`, `userPromptSubmitted`), and its payloads then carry no
    /// event name, so the helper adds the registered one; both spellings read the same.
    fn event(self, name: &str) -> &str {
        match (self, name) {
            (Self::Copilot, "userPromptSubmitted") => "UserPromptSubmit",
            (Self::Copilot, "postToolUse") => "PostToolUse",
            (Self::Copilot, "notification") => "Notification",
            (Self::Copilot, "agentStop") => "Stop",
            (Self::Copilot, "sessionEnd") => "SessionEnd",
            _ => name,
        }
    }

    /// The session's name in Knock: Claude Code's own ids stay as they are,
    /// another agent's get its id in front, so two agents never share one.
    fn session(self, raw: String) -> Result<String, ParseError> {
        let id = match self {
            Self::Claude => raw,
            other => format!("{}-{raw}", other.id()),
        };
        is_id(&id).then_some(id).ok_or(ParseError::BadSessionId)
    }
}

impl Adapter for Agy {
    fn id(&self) -> &'static str {
        "agy"
    }
    fn display_name(&self) -> &'static str {
        "Antigravity"
    }
    /// Only a guess at permission prompts (see [`parse_agy`]), and no end of session.
    fn capabilities(&self) -> &'static [Capability] {
        &[
            Capability::NeedsPermission,
            Capability::Failed,
            Capability::Idle,
        ]
    }
    /// All six agy events; the app knows agy's file layout (flat lists for the
    /// lifecycle events, matcher groups for the two tool events).
    fn hooks(&self) -> HookSpec {
        HookSpec {
            path: "/v1/hooks/agy",
            events: &[
                ("SessionStart", None),
                ("PreInvocation", None),
                ("PreToolUse", Some("*")),
                ("PostToolUse", Some("*")),
                ("PostInvocation", None),
                ("Stop", None),
            ],
        }
    }
    fn parse(&self, body: &[u8], ts: u64) -> Result<Option<Event>, ParseError> {
        parse_agy(&self.users, body, ts)
    }
}

/// A field of the wrong type reads as absent instead of failing the whole payload:
/// agy is a program Knock doesn't control, and a surprise in one field shouldn't
/// hide the rest.
fn lenient<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> Result<Option<T>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

/// The allow-list of an agy payload: names, numbers and flags only. The prompt
/// (`lastUserInput`), model output, transcript paths and tool arguments are
/// never kept; of the arguments only `BypassSandbox` is read.
#[derive(Deserialize)]
struct AgyRaw {
    #[serde(
        default,
        rename = "hook_event_name",
        deserialize_with = "string_or_none"
    )]
    event: Option<String>,
    #[serde(
        default,
        rename = "conversationId",
        deserialize_with = "string_or_none"
    )]
    conversation_id: Option<String>,
    #[serde(
        default,
        rename = "parentConversationId",
        deserialize_with = "string_or_none"
    )]
    parent: Option<String>,
    #[serde(default, rename = "workspacePaths", deserialize_with = "lenient")]
    workspace_paths: Option<Vec<Option<String>>>,
    #[serde(default, rename = "invocationNum", deserialize_with = "lenient")]
    invocation_num: Option<u64>,
    #[serde(default, rename = "toolCall", deserialize_with = "lenient")]
    tool_call: Option<AgyTool>,
    #[serde(
        default,
        rename = "terminationReason",
        deserialize_with = "string_or_none"
    )]
    termination: Option<String>,
    #[serde(default, rename = "fullyIdle", deserialize_with = "lenient")]
    fully_idle: Option<bool>,
}

#[derive(Deserialize)]
struct AgyTool {
    #[serde(default, deserialize_with = "string_or_none")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    args: Option<AgyArgs>,
}

#[derive(Deserialize)]
struct AgyArgs {
    #[serde(default, rename = "BypassSandbox", deserialize_with = "lenient")]
    bypass_sandbox: Option<bool>,
}

/// agy's events, as Knock's (ADR 0039).
/// - The first model call of a turn (`SessionStart`, `PreInvocation` number 0)
///   starts a turn; the others are activity.
/// - `PreToolUse` of an unsandboxed `run_command` is the one case agy documents as
///   waiting for the user's approval, so it reads as a permission request. It is a
///   guess: a command the user allowed beforehand doesn't wait, and the next event
///   (`PostToolUse`) clears it at once. agy has no event for "waiting for approval".
/// - `Stop` ends the turn: an error fails it, the user cancelling leaves the agent
///   idle, and while background tasks run (`fullyIdle` false) it's still activity.
/// - A subagent's conversation (it has a parent) isn't a session of its own.
fn parse_agy(users: &[String], body: &[u8], ts: u64) -> Result<Option<Event>, ParseError> {
    let raw: AgyRaw = serde_json::from_slice(body).map_err(|_| ParseError::BadPayload)?;
    let conversation = raw
        .conversation_id
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::NoSessionId)?;
    if !is_id(&conversation) {
        return Err(ParseError::BadSessionId);
    }
    let session_id = Dialect::Agy.session(conversation)?;
    if raw.parent.is_some_and(|p| !p.is_empty()) {
        return Ok(None);
    }
    let tool = raw
        .tool_call
        .as_ref()
        .and_then(|t| t.name.clone())
        .map(|t| {
            if charset(&t, 128, |c| c.is_ascii_alphanumeric() || "_.:-".contains(c)) {
                t
            } else {
                "other".into()
            }
        });
    let activity = |tool: Option<String>| Detail {
        category: tool.as_deref().map(Category::of),
        tool,
        ..Detail::default()
    };
    let (kind, detail) = match raw.event.as_deref().unwrap_or("") {
        "SessionStart" => (Kind::TurnStarted, Detail::default()),
        "PreInvocation" if raw.invocation_num == Some(0) => (Kind::TurnStarted, Detail::default()),
        "PreInvocation" | "PostInvocation" => (Kind::Activity, Detail::default()),
        "PreToolUse" => {
            let unsandboxed = raw
                .tool_call
                .as_ref()
                .and_then(|t| t.args.as_ref())
                .and_then(|a| a.bypass_sandbox)
                == Some(true);
            if tool.as_deref() == Some("run_command") && unsandboxed {
                (
                    Kind::NeedsPermission,
                    Detail {
                        tool,
                        ..Detail::default()
                    },
                )
            } else {
                (Kind::Activity, activity(tool))
            }
        }
        "PostToolUse" => (Kind::Activity, activity(tool)),
        "Stop" => match raw.termination.as_deref().unwrap_or("") {
            "ERROR" => (
                Kind::AgentFailed,
                Detail {
                    reason: Some("error".into()),
                    ..Detail::default()
                },
            ),
            "USER_CANCELED" => (Kind::TurnIdle, Detail::default()),
            _ if raw.fully_idle == Some(false) => (Kind::Activity, Detail::default()),
            _ => (Kind::TurnCompleted, Detail::default()),
        },
        _ => return Ok(None),
    };
    let cwd = (raw.workspace_paths.unwrap_or_default().into_iter())
        .flatten()
        .next()
        .unwrap_or_default();
    Ok(Some(Event {
        v: 1,
        agent: "agy",
        session_id,
        ts,
        kind,
        detail,
        context: Context {
            project: project_with(&cwd, users).into(),
            cwd,
            pid: None,
            agent_id: None,
        },
    }))
}

impl Adapter for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn display_name(&self) -> &'static str {
        "Codex"
    }
    fn capabilities(&self) -> &'static [Capability] {
        &[Capability::NeedsPermission, Capability::Idle]
    }
    /// No `PreToolUse` or `UserPromptSubmit`-blocking hooks: Knock only
    /// watches, and these are the events where a failing hook could block
    /// the agent. `PermissionRequest` runs only when Codex is about to ask.
    fn hooks(&self) -> HookSpec {
        HookSpec {
            path: "/v1/hooks/codex",
            events: &[
                ("UserPromptSubmit", None),
                ("PostToolUse", Some("*")),
                ("PermissionRequest", Some("*")),
                ("Stop", None),
                ("Interrupt", None),
                ("SessionEnd", None),
            ],
        }
    }
    fn parse(&self, body: &[u8], ts: u64) -> Result<Option<Event>, ParseError> {
        parse_with(Dialect::Codex, &self.users, body, ts)
    }
}

impl Adapter for Copilot {
    fn id(&self) -> &'static str {
        "copilot"
    }
    fn display_name(&self) -> &'static str {
        "GitHub Copilot"
    }
    fn capabilities(&self) -> &'static [Capability] {
        &[
            Capability::NeedsPermission,
            Capability::NeedsAnswer,
            Capability::Idle,
        ]
    }
    /// Never `preToolUse` (a failing hook there denies the tool) and never
    /// `permissionRequest` (it runs even when nothing asks the user); the
    /// `notification` for a permission prompt is the real signal. The camelCase
    /// names are the ones Copilot's hook files document; the helper adds the
    /// name to the payload, and both payload styles are read ([`Dialect::event`]).
    fn hooks(&self) -> HookSpec {
        HookSpec {
            path: "/v1/hooks/copilot",
            events: &[
                ("userPromptSubmitted", None),
                ("postToolUse", None),
                ("notification", None),
                ("agentStop", None),
                ("sessionEnd", None),
            ],
        }
    }
    fn parse(&self, body: &[u8], ts: u64) -> Result<Option<Event>, ParseError> {
        parse_with(Dialect::Copilot, &self.users, body, ts)
    }
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
        parse_with(Dialect::Claude, &self.users, body, ts)
    }
}

/// The one parser for every agent whose hooks send Claude Code's payloads.
fn parse_with(
    dialect: Dialect,
    users: &[String],
    body: &[u8],
    ts: u64,
) -> Result<Option<Event>, ParseError> {
    {
        let raw: Raw = serde_json::from_slice(body).map_err(|_| ParseError::BadPayload)?;
        let session_id = raw
            .session_id
            .filter(|s| !s.is_empty())
            .ok_or(ParseError::NoSessionId)?;
        if !is_id(&session_id) {
            return Err(ParseError::BadSessionId);
        }
        let session_id = dialect.session(session_id)?;
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
        let event = dialect.event(raw.hook_event_name.as_deref().unwrap_or(""));
        let (kind, detail) = match event {
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
            // The user stopped the turn: the agent waits for the next prompt.
            "Interrupt" if dialect == Dialect::Codex => (Kind::TurnIdle, Detail::default()),
            _ => return Ok(None),
        };
        let cwd = raw.cwd.unwrap_or_default();
        Ok(Some(Event {
            v: 1,
            agent: dialect.id(),
            session_id,
            ts,
            kind,
            detail,
            context: Context {
                project: project_with(&cwd, users).into(),
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
