//! "While you were away" (SPEC.md FR-9, §9.7 Idle time, §10.3; ADR 0034).
//! Pure: the app passes the time, the user's idle time and the sessions on
//! every tick; nothing here reads a clock or the OS.
//!
//! - **Away** starts once the idle time reaches the threshold (10 minutes,
//!   [`AWAY_MS`]). The idle period began at the last input, `now - idle`,
//!   which is the summary's `since`, not the moment the threshold was reached.
//! - **Return** is the first tick whose idle time is lower than the tick
//!   before: input was seen again. Only the idle times are compared, both
//!   from the same OS counter, so a wall-clock change can't fake a return.
//! - **Items** (ADR 0034): every session whose current state is `ready`,
//!   `failed`, `needs_input` or `stalled` and was entered at or after `since`
//!   ([`Session::since`] is when the state was entered). That covers a
//!   session that entered one of them during the idle period and is still
//!   there, one that started during it, and one that moved from one of them
//!   to another. One that left again, one that was removed, and one that was
//!   there before `since` and never left are not listed. The outcome is the
//!   state at return. Simulated sessions count like real ones.
//! - **Exception** (ADR 0034): a `needs_input` session whose current episode
//!   opened at or after `since` ([`crate::Needs::opened`]) is listed even if
//!   it was already in `needs_input` (seen, ADR 0016) before. A new request
//!   that only joins an episode opened earlier doesn't count.
//! - No items: no summary.

use crate::engine::{Session, State};

/// §9.7: 10 minutes or more without input counts as away.
pub const AWAY_MS: u64 = 10 * 60_000;

/// §8.5 `away-summary` `outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ready,
    Failed,
    NeedsYou,
}

impl Outcome {
    /// ADR 0034: `stalled` and `needs_input` are both "needs_you".
    pub fn of(state: State) -> Option<Self> {
        match state {
            State::Ready => Some(Self::Ready),
            State::Failed => Some(Self::Failed),
            State::NeedsInput | State::Stalled => Some(Self::NeedsYou),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::NeedsYou => "needs_you",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwayItem {
    pub id: String,
    pub project: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwaySummary {
    /// When the idle period began (the last input), ms since the Unix epoch.
    pub since: u64,
    /// In the order the sessions first appeared.
    pub items: Vec<AwayItem>,
}

#[derive(Debug, Clone)]
pub struct Away {
    threshold: u64,
    /// The idle period's start while the user is away.
    since: Option<u64>,
    /// The idle time at the last tick.
    last_idle: u64,
}

impl Away {
    /// `threshold_ms`: [`AWAY_MS`], or shorter for manual tests.
    pub fn new(threshold_ms: u64) -> Self {
        Away {
            threshold: threshold_ms,
            since: None,
            last_idle: 0,
        }
    }

    /// The idle period's start while the user is away.
    pub fn since(&self) -> Option<u64> {
        self.since
    }

    /// Once a tick, after the engine's tick, so `sessions` are the states at
    /// this time. `idle_ms`: time since the user's last input. Returns the
    /// summary on the tick the user returns, if any session qualifies.
    pub fn tick(&mut self, now: u64, idle_ms: u64, sessions: &[Session]) -> Option<AwaySummary> {
        let input = idle_ms < self.last_idle;
        self.last_idle = idle_ms;
        let summary = match self.since {
            Some(since) if input => {
                self.since = None;
                summary(since, sessions)
            }
            _ => None,
        };
        // Also right after a return, in case the user came and left again
        // between two ticks (only if ticks stopped, as while the PC slept).
        if self.since.is_none() && idle_ms >= self.threshold {
            self.since = Some(now.saturating_sub(idle_ms));
        }
        summary
    }
}

/// ADR 0034 items for an idle period that began at `since`; `None` if empty.
pub fn summary(since: u64, sessions: &[Session]) -> Option<AwaySummary> {
    let items: Vec<_> = sessions
        .iter()
        .filter(|s| {
            // The ADR 0034 exception: a new episode replacing a seen one
            // keeps `needs_input` (and `since`), but has its own `opened`.
            let reopened = s.needs.as_ref().is_some_and(|n| n.opened >= since);
            s.since >= since || reopened
        })
        .filter_map(|s| {
            Some(AwayItem {
                id: s.id.clone(),
                project: s.project.clone(),
                outcome: Outcome::of(s.state)?,
            })
        })
        .collect();
    (!items.is_empty()).then_some(AwaySummary { since, items })
}
