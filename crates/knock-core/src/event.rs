//! Normalized event, version 1 (SPEC.md §8.3). Serializes to the §8.3 shape.

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub v: u8,
    pub agent: &'static str,
    pub session_id: String,
    /// Server receive time, ms since the Unix epoch; serialized as ISO 8601.
    #[serde(serialize_with = "ser_ts")]
    pub ts: u64,
    pub kind: Kind,
    pub detail: Detail,
    pub context: Context,
}

impl Event {
    /// A Knock-internal event (`user.ack`, `user.dismiss`) for a session.
    pub fn user(kind: Kind, session_id: &str, ts: u64) -> Self {
        Event {
            v: 1,
            agent: "claude-code",
            session_id: session_id.into(),
            ts,
            kind,
            detail: Detail::default(),
            context: Context::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "activity")]
    Activity,
    #[serde(rename = "needs.permission")]
    NeedsPermission,
    #[serde(rename = "needs.answer")]
    NeedsAnswer,
    #[serde(rename = "turn.idle")]
    TurnIdle,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
    #[serde(rename = "agent.failed")]
    AgentFailed,
    #[serde(rename = "session.ended")]
    SessionEnded,
    #[serde(rename = "user.ack")]
    UserAck,
    #[serde(rename = "user.dismiss")]
    UserDismiss,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// `agent.failed` only: the `StopFailure` error code, never text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `session.started` only; no producer in v1 (ADR 0005).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub cwd: String,
    pub project: String,
    /// Set by the app when known (§9.4); the adapter never knows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Read,
    Search,
    Edit,
    Run,
    Web,
    Other,
}

impl Category {
    /// §8.3 tool categories.
    pub fn of(tool: &str) -> Self {
        match tool {
            "Read" | "NotebookRead" => Self::Read,
            "Grep" | "Glob" => Self::Search,
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Self::Edit,
            "Bash" | "PowerShell" => Self::Run,
            "WebFetch" | "WebSearch" => Self::Web,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Startup,
    Resume,
    Clear,
    Compact,
}

fn ser_ts<S: Serializer>(ms: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&iso8601(*ms))
}

/// UTC ISO 8601 with milliseconds, e.g. `2026-10-16T14:02:11.120Z`
/// (days-to-civil from H. Hinnant's public-domain date algorithms).
pub fn iso8601(ms: u64) -> String {
    let rem = ms % 86_400_000;
    let z = (ms / 86_400_000) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}
