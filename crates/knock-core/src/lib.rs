//! Knock's pure core: normalized events, the Claude Code adapter, the state
//! engine, timers and away detection (SPEC.md §7.1). No I/O, no OS calls, no
//! Tauri, no wall clock (§7.3): time comes in only as `Input::Tick`, the
//! event `ts` and the idle time passed to [`Away::tick`].

mod adapter;
mod away;
mod engine;
mod event;
pub mod packs;
pub mod paths;

pub use adapter::{
    Adapter, Capability, ClaudeCode, HookSpec, ParseError, project_key, project_with,
};
pub use away::{AWAY_MS, Away, AwayItem, AwaySummary, Outcome, summary};
pub use engine::{
    ANIMALS, Effect, Engine, Input, Needs, NeedsKind, Reason, Session, Settings, State, Tier,
};
pub use event::{Category, Context, Detail, Event, Kind, Source, iso8601};
