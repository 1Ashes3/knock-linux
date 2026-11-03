//! Knock's pure core: normalized events, the Claude Code adapter, the state
//! engine and timers (SPEC.md §7.1). No I/O, no OS calls, no Tauri, no wall
//! clock (§7.3): time comes in only as `Input::Tick` and the event `ts`.

mod adapter;
mod engine;
mod event;

pub use adapter::{
    Adapter, Capability, ClaudeCode, HookSpec, ParseError, project_key, project_with,
};
pub use engine::{
    ANIMALS, Effect, Engine, Input, Needs, NeedsKind, Reason, Session, Settings, State, Tier,
};
pub use event::{Category, Context, Detail, Event, Kind, Source, iso8601};
