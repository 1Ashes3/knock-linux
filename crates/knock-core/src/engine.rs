//! The state engine (SPEC.md §8.4; ADR 0010, 0016, 0018, 0019, 0021). One `step`
//! per input; every effect it returns belongs to that input's time.
//!
//! Inputs the app provides instead of the engine reading them: the clock
//! (`Tick`), whether notifications are suppressed (`SetSuppressed`: pause or
//! quiet hours, worked out by the app from local time; sent before the first
//! event and on every change), process liveness (`ProcessGone`) and the user
//! actions (`user.ack`, `user.dismiss`).
//!
//! Requesters (ADR 0019): the agent that asked, `None` for the main agent or
//! `Some(agentId)` for a sub-agent. An episode holds every requester still
//! waiting and closes when none is left. If the main agent's turn already
//! completed (ADR 0021), it closes to `ready`, not `working`.
//!
//! Known process (ADR 0018 item 7): an event's `context.pid` is set by the
//! app exactly when it found the session's process (§8.3, §9.4). A session
//! none of whose events carried a pid has no known process, so it counts as
//! having no live process and is removed 6 h after its last event. Once a pid
//! arrives, only `ProcessGone` starts that clock. Simulated sessions never get
//! a pid (§8.1), so they follow the no-process rule.
//!
//! A (state, input) pair with no §8.4 row leaves the session unchanged.

use crate::adapter::project_key;
use crate::event::{Category, Event, Kind};
use std::collections::HashMap;

const DEDUP_MS: u64 = 10_000;
const FADE_MS: u64 = 60_000;
const REMOVE_MS: u64 = 6 * 3_600_000;

/// The v1 animal ids, in assignment order (§8.7, ADR 0030).
pub const ANIMALS: [&str; 4] = ["fox", "rabbit", "cat", "owl"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// §8.6 `stallMinutes`, in ms.
    pub stall_ms: u64,
    /// §8.6 `escalationMinutes`, in ms.
    pub escalation_ms: u64,
    pub notify_on_stall: bool,
    /// Animal ids in assignment order; [`ANIMALS`] by default.
    pub animals: Vec<String>,
    /// §8.6 `animalOverrides`: [`project_key`] → animal id.
    pub animal_overrides: HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            stall_ms: 10 * 60_000,
            escalation_ms: 5 * 60_000,
            notify_on_stall: false,
            animals: ANIMALS.map(String::from).to_vec(),
            animal_overrides: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// `simulated`: the request carried `X-Knock-Sim: 1` (§8.1).
    Event { event: Event, simulated: bool },
    /// Once a second.
    Tick { now: u64 },
    /// `pauseNotifications` is on or it is quiet hours. Holds from this input
    /// on, until the next one; starts `false`. The app sends it before the
    /// first event and whenever the value changes, never waiting for a tick.
    SetSuppressed(bool),
    /// The session's recorded process is no longer alive (§9.7).
    ProcessGone { session_id: String, now: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Notify {
        session_id: String,
        reason: Reason,
    },
    Escalate {
        session_id: String,
    },
    ReadyBatchAdd {
        session_id: String,
    },
    /// The snapshot (sessions, states, episodes) changed; save it.
    Persist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NeedsPermission,
    NeedsAnswer,
    Failed,
    Stalled,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsPermission => "needs_permission",
            Self::NeedsAnswer => "needs_answer",
            Self::Failed => "failed",
            Self::Stalled => "stalled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    NeedsInput,
    Failed,
    Stalled,
    /// Unused in v1.
    BlockedFixable,
    Ready,
    Working,
    /// Unused in v1.
    WaitingAgent,
    /// Unused in v1.
    Communicating,
    Idle,
    EndedUnexpectedly,
    /// Unused in v1.
    Paused,
    /// Unused in v1.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    NeedsYou,
    Ready,
    Working,
    Resting,
}

/// Every state, for [`State::parse`].
const STATES: [State; 12] = [
    State::NeedsInput,
    State::Failed,
    State::Stalled,
    State::BlockedFixable,
    State::Ready,
    State::Working,
    State::WaitingAgent,
    State::Communicating,
    State::Idle,
    State::EndedUnexpectedly,
    State::Paused,
    State::Cancelled,
];

impl State {
    /// The state named `s` ([`State::as_str`]), as `sessions.json` saves it.
    pub fn parse(s: &str) -> Option<Self> {
        STATES.into_iter().find(|st| st.as_str() == s)
    }

    pub fn tier(self) -> Tier {
        use State::*;
        match self {
            NeedsInput | Failed | Stalled | BlockedFixable => Tier::NeedsYou,
            Ready => Tier::Ready,
            Working | WaitingAgent | Communicating => Tier::Working,
            Idle | EndedUnexpectedly | Paused | Cancelled => Tier::Resting,
        }
    }
    pub fn as_str(self) -> &'static str {
        use State::*;
        match self {
            NeedsInput => "needs_input",
            Failed => "failed",
            Stalled => "stalled",
            BlockedFixable => "blocked_fixable",
            Ready => "ready",
            Working => "working",
            WaitingAgent => "waiting_agent",
            Communicating => "communicating",
            Idle => "idle",
            EndedUnexpectedly => "ended_unexpectedly",
            Paused => "paused",
            Cancelled => "cancelled",
        }
    }
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Ready => "ready",
            Self::Working => "working",
            Self::Resting => "resting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedsKind {
    Permission,
    Answer,
}

impl NeedsKind {
    /// `permission` or `answer`, as `sessions.json` saves it.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "permission" => Some(Self::Permission),
            "answer" => Some(Self::Answer),
            _ => None,
        }
    }
}

/// FR-13: a session as `sessions.json` saved it, to bring back after the app
/// restarts. The app passes only sessions whose process it found still
/// running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    /// One of [`crate::AGENTS`]; a name no longer known comes back as Claude Code.
    pub agent: String,
    pub project: String,
    pub project_key: String,
    pub state: State,
    pub since: u64,
    pub last_event: u64,
    pub activity: Option<Category>,
    pub needs: Option<SavedNeeds>,
    pub animal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedNeeds {
    pub kind: NeedsKind,
    pub label: Option<String>,
    pub seen: bool,
    pub opened: u64,
    pub escalated: bool,
}

/// The open `needs_input` episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needs {
    /// Set by the event that opened the episode; never changes (ADR 0010).
    pub kind: NeedsKind,
    /// The tool, for a permission; `None` for a question or when unknown.
    pub label: Option<String>,
    /// The user clicked or jumped to it (ADR 0016).
    pub seen: bool,
    pub opened: u64,
    pub escalated: bool,
    /// The requesters still waiting (ADR 0019); never empty. Kept only for
    /// the episode.
    waiting: Vec<Option<String>>,
}

impl Needs {
    /// The requesters still waiting: `None` is the main agent.
    pub fn waiting(&self) -> &[Option<String>] {
        &self.waiting
    }
}

/// Everything `SessionView` (§8.5) needs except precision, which is app-side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    /// The agent whose hooks reported it (`claude-code`, `codex`, `copilot`).
    pub agent: &'static str,
    pub project: String,
    /// [`project_key`] of the first event's `cwd`; the `cwd` itself is not kept (§11.4).
    pub project_key: String,
    pub state: State,
    /// When `state` was entered.
    pub since: u64,
    pub activity: Option<Category>,
    /// `Some` exactly when `state` is `needs_input`.
    pub needs: Option<Needs>,
    pub animal: String,
    pub simulated: bool,
    /// Last agent event (user actions don't count).
    pub last_event: u64,
    gone_at: Option<u64>,
    /// Some event carried a pid (see the module docs).
    process_known: bool,
    /// Last request-type `needs.*` per requester, for the dedup; only the
    /// last 10 s are kept.
    requests: Vec<(Option<String>, u64)>,
    /// The main agent's turn completed and it hasn't asked or worked since
    /// (ADR 0021).
    turn_done: bool,
}

impl Session {
    /// A session as its first event creates it (also used to restart an
    /// `ended_unexpectedly` one, ADR 0018).
    fn new(e: &Event, simulated: bool, animal: String) -> Self {
        Session {
            id: e.session_id.clone(),
            agent: e.agent,
            project: e.context.project.clone(),
            project_key: project_key(&e.context.cwd),
            state: State::Idle,
            since: e.ts,
            activity: None,
            needs: None,
            animal,
            simulated,
            last_event: e.ts,
            gone_at: None,
            process_known: e.context.pid.is_some(),
            requests: Vec::new(),
            turn_done: false,
        }
    }

    fn set(&mut self, to: State, now: u64) {
        if self.state != to {
            self.state = to;
            self.since = now;
        }
        if to != State::NeedsInput {
            self.needs = None;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Engine {
    settings: Settings,
    suppressed: bool,
    sessions: Vec<Session>,
}

type Snapshot = Vec<(String, State, Option<Needs>)>;

impl Engine {
    pub fn new(settings: Settings) -> Self {
        Engine {
            settings,
            ..Self::default()
        }
    }

    /// FR-13: brings back sessions saved before a restart, as they were:
    /// state, since, activity, an open episode (seen and escalated as saved,
    /// so nothing knocks or reminds again) and the animal, if it is still in
    /// the cast and free. Sessions already known are skipped; a saved
    /// `needs_input` without its episode comes back idle. Emits nothing:
    /// nothing new happened. The app's liveness check ends any that stop.
    pub fn restore(&mut self, saved: Vec<Saved>) {
        for s in saved {
            if self.sessions.iter().any(|x| x.id == s.id) {
                continue;
            }
            let (state, needs) = match (s.state, s.needs) {
                (State::NeedsInput, Some(n)) => (
                    State::NeedsInput,
                    Some(Needs {
                        kind: n.kind,
                        label: n.label,
                        seen: n.seen,
                        opened: n.opened,
                        escalated: n.escalated,
                        // The main agent: who asked isn't saved (ADR 0019).
                        waiting: vec![None],
                    }),
                ),
                (State::NeedsInput, None) => (State::Idle, None),
                (state, _) => (state, None),
            };
            let cast = self.settings.animals.contains(&s.animal)
                || self.settings.animal_overrides.get(&s.project_key) == Some(&s.animal);
            let free = self.sessions.iter().all(|x| x.animal != s.animal);
            let animal = if cast && free {
                s.animal
            } else {
                self.animal_for(&s.project_key, self.sessions.len())
            };
            let agent = crate::AGENTS
                .iter()
                .find(|a| **a == s.agent)
                .copied()
                .unwrap_or("claude-code");
            self.sessions.push(Session {
                id: s.id,
                agent,
                project: s.project,
                project_key: s.project_key,
                state,
                since: s.since,
                activity: s.activity,
                needs,
                animal,
                simulated: false,
                last_event: s.last_event,
                gone_at: None,
                process_known: true,
                requests: Vec::new(),
                turn_done: state == State::Ready,
            });
        }
    }

    /// A new animal list (another sprite pack, ADR 0038) or new overrides
    /// re-cast every session in its order, as if it had just started; any
    /// other change leaves the animals alone.
    pub fn set_settings(&mut self, settings: Settings) {
        let recast = settings.animals != self.settings.animals
            || settings.animal_overrides != self.settings.animal_overrides;
        self.settings = settings;
        if recast {
            for s in &mut self.sessions {
                s.animal.clear();
            }
            for i in 0..self.sessions.len() {
                let animal = self.animal_for(&self.sessions[i].project_key, i);
                self.sessions[i].animal = animal;
            }
        }
    }

    /// In the order they first appeared.
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn step(&mut self, input: Input) -> Vec<Effect> {
        let before = self.snapshot();
        let mut fx = Vec::new();
        match input {
            Input::Event { event, simulated } => self.event(event, simulated, &mut fx),
            Input::Tick { now } => self.tick(now, &mut fx),
            Input::SetSuppressed(on) => self.suppressed = on,
            Input::ProcessGone { session_id, now } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.gone_at.get_or_insert(now);
                    if s.state != State::Failed {
                        s.set(State::EndedUnexpectedly, now);
                    }
                }
            }
        }
        if self.snapshot() != before {
            fx.push(Effect::Persist);
        }
        fx
    }

    fn snapshot(&self) -> Snapshot {
        let s = self.sessions.iter();
        s.map(|s| (s.id.clone(), s.state, s.needs.clone()))
            .collect()
    }

    fn notify(&self, fx: &mut Vec<Effect>, id: &str, reason: Reason) {
        if !self.suppressed {
            fx.push(Effect::Notify {
                session_id: id.into(),
                reason,
            });
        }
    }

    /// The animal for a session with project `key` that is the `n`th to
    /// get one (0-based): its override, else the first one no session has,
    /// else round-robin by `n`.
    fn animal_for(&self, key: &str, n: usize) -> String {
        if let Some(a) = self.settings.animal_overrides.get(key) {
            return a.clone();
        }
        let all = &self.settings.animals;
        let free = all
            .iter()
            .find(|a| self.sessions.iter().all(|s| &s.animal != *a));
        free.or_else(|| all.get(n % all.len().max(1)))
            .cloned()
            .unwrap_or_default()
    }

    fn event(&mut self, e: Event, simulated: bool, fx: &mut Vec<Effect>) {
        let (ts, kind) = (e.ts, e.kind);
        let user = matches!(kind, Kind::UserAck | Kind::UserDismiss);
        let restart = !user && kind != Kind::SessionEnded;
        let i = match self.sessions.iter().position(|s| s.id == e.session_id) {
            // ADR 0018: an agent event continues an ended_unexpectedly
            // session (e.g. `--resume` in a new terminal) as a first event,
            // keeping its animal and its place.
            Some(i) if restart && self.sessions[i].state == State::EndedUnexpectedly => {
                let animal = std::mem::take(&mut self.sessions[i].animal);
                self.sessions[i] = Session::new(&e, simulated, animal);
                return self.first(i, e, fx);
            }
            Some(i) => i,
            None if !restart => return,
            None => {
                let animal = self.animal_for(&project_key(&e.context.cwd), self.sessions.len());
                self.sessions.push(Session::new(&e, simulated, animal));
                return self.first(self.sessions.len() - 1, e, fx);
            }
        };
        let suppressed = self.suppressed;
        let s = &mut self.sessions[i];
        if !user {
            s.last_event = ts;
            s.process_known |= e.context.pid.is_some();
        }
        use State::*;
        match (kind, s.state) {
            (Kind::TurnStarted, Idle | Ready | NeedsInput | Stalled | Failed) => {
                s.activity = None;
                s.turn_done = false;
                s.set(Working, ts);
            }
            // ADR 0019: background activity leaves Failed as it is.
            (Kind::Activity, Failed) if e.context.agent_id.is_some() => {}
            (Kind::Activity, Working | Stalled | Failed) => {
                s.turn_done &= e.context.agent_id.is_some();
                s.activity = e.detail.category.or(s.activity);
                s.set(Working, ts);
            }
            // Removes that requester only; closes when none is left (ADR
            // 0019), to ready if the main agent's turn is done (ADR 0021).
            (Kind::Activity, NeedsInput) => {
                if remove(s, &e.context.agent_id) == 0 {
                    if s.turn_done {
                        s.set(Ready, ts);
                        fx.push(Effect::ReadyBatchAdd {
                            session_id: s.id.clone(),
                        });
                    } else {
                        s.activity = e.detail.category.or(s.activity);
                        s.set(Working, ts);
                    }
                }
            }
            (Kind::NeedsPermission | Kind::NeedsAnswer, from) => {
                let dup = dedup(s, &e);
                let who = &e.context.agent_id;
                // The main agent asking again: its turn isn't done (ADR 0021).
                s.turn_done &= dup || who.is_some();
                // `Some(waiting)`: open a new episode that also waits on them.
                let opens = match (from, &mut s.needs) {
                    _ if dup => None,
                    (Working | Idle | Ready | Stalled | Failed, _) => Some(Vec::new()),
                    // ADR 0018 item 1: a new request (never the
                    // `permission_prompt` Notification) after the user saw
                    // the episode is a new one; the old requesters still wait.
                    (NeedsInput, Some(n)) if n.seen && !is_prompt(&e) => {
                        Some(std::mem::take(&mut n.waiting))
                    }
                    // Otherwise the same episode; the requester is added.
                    (NeedsInput, Some(n)) => {
                        if !n.waiting.contains(who) {
                            n.waiting.push(who.clone());
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(waiting) = opens {
                    self.open(i, e, waiting, fx);
                }
            }
            // The Stop removes the main agent only (ADR 0019).
            (Kind::TurnCompleted, NeedsInput | Working | Stalled) => {
                s.turn_done = true;
                if remove(s, &None) == 0 {
                    s.set(Ready, ts);
                    fx.push(Effect::ReadyBatchAdd {
                        session_id: s.id.clone(),
                    });
                }
            }
            (Kind::AgentFailed, from) => {
                s.set(Failed, ts);
                if from != Failed && !suppressed {
                    fx.push(Effect::Notify {
                        session_id: s.id.clone(),
                        reason: Reason::Failed,
                    });
                }
            }
            (Kind::SessionEnded, _) | (Kind::UserDismiss, Failed | EndedUnexpectedly) => {
                self.sessions.remove(i);
            }
            (Kind::UserAck, NeedsInput) => {
                if let Some(n) = &mut s.needs {
                    n.seen = true;
                }
            }
            (Kind::UserAck, Ready) => s.set(Idle, ts),
            _ => {}
        }
    }

    /// The first event of session `i`: `idle`, or the state the event implies.
    fn first(&mut self, i: usize, e: Event, fx: &mut Vec<Effect>) {
        let ts = e.ts;
        match e.kind {
            Kind::TurnStarted => self.sessions[i].set(State::Working, ts),
            Kind::Activity => {
                self.sessions[i].activity = e.detail.category;
                self.sessions[i].set(State::Working, ts);
            }
            Kind::NeedsPermission | Kind::NeedsAnswer => {
                dedup(&mut self.sessions[i], &e);
                self.open(i, e, Vec::new(), fx);
            }
            Kind::TurnCompleted => {
                self.sessions[i].turn_done = true;
                self.sessions[i].set(State::Ready, ts);
                fx.push(Effect::ReadyBatchAdd {
                    session_id: e.session_id,
                });
            }
            Kind::AgentFailed => {
                self.sessions[i].set(State::Failed, ts);
                self.notify(fx, &e.session_id, Reason::Failed);
            }
            _ => {}
        }
    }

    /// New `needs_input` episode, with its one `Notify`, waiting on
    /// `waiting` and the event's requester.
    fn open(&mut self, i: usize, e: Event, mut waiting: Vec<Option<String>>, fx: &mut Vec<Effect>) {
        if !waiting.contains(&e.context.agent_id) {
            waiting.push(e.context.agent_id.clone());
        }
        let (kind, reason) = match e.kind {
            Kind::NeedsAnswer => (NeedsKind::Answer, Reason::NeedsAnswer),
            _ => (NeedsKind::Permission, Reason::NeedsPermission),
        };
        let s = &mut self.sessions[i];
        s.set(State::NeedsInput, e.ts);
        s.needs = Some(Needs {
            kind,
            label: e.detail.tool.filter(|_| kind == NeedsKind::Permission),
            seen: false,
            opened: e.ts,
            escalated: false,
            waiting,
        });
        self.notify(fx, &e.session_id, reason);
    }

    fn tick(&mut self, now: u64, fx: &mut Vec<Effect>) {
        let (stall, escalation) = (self.settings.stall_ms, self.settings.escalation_ms);
        let (suppressed, notify_stall) = (self.suppressed, self.settings.notify_on_stall);
        self.sessions.retain(|s| {
            let faded =
                s.state == State::EndedUnexpectedly && now.saturating_sub(s.since) >= FADE_MS;
            // No live process: reported gone, or never known (ADR 0018).
            let gone = s.gone_at.or((!s.process_known).then_some(0));
            let quiet = gone.is_some_and(|g| now.saturating_sub(g.max(s.last_event)) >= REMOVE_MS);
            !faded && !quiet
        });
        for s in &mut self.sessions {
            match s.state {
                State::Working if now.saturating_sub(s.last_event) >= stall => {
                    s.set(State::Stalled, now);
                    if notify_stall && !suppressed {
                        fx.push(Effect::Notify {
                            session_id: s.id.clone(),
                            reason: Reason::Stalled,
                        });
                    }
                }
                State::NeedsInput => {
                    let Some(n) = &mut s.needs else { continue };
                    if !n.escalated && !n.seen && now.saturating_sub(n.opened) >= escalation {
                        // Suppressed means dropped, not deferred.
                        n.escalated = true;
                        if !suppressed {
                            fx.push(Effect::Escalate {
                                session_id: s.id.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The 10 s dedup (§8.4, ADR 0010; per requester, ADR 0019): a
/// `permission_prompt` Notification (the `needs.permission` with no tool)
/// within 10 s after a PermissionRequest from the same requester belongs to
/// that request's episode, even if it closed. A prompt with no `agentId`
/// matches any requester's request (ADR 0021). Every other `needs.*` counts as
/// the request half and is never a duplicate itself, so a second request
/// right after a prompt still knocks.
fn dedup(s: &mut Session, e: &Event) -> bool {
    let who = &e.context.agent_id;
    s.requests
        .retain(|(_, t)| e.ts.saturating_sub(*t) <= DEDUP_MS);
    if is_prompt(e) {
        s.requests.iter().any(|(a, _)| who.is_none() || a == who)
    } else {
        s.requests.retain(|(a, _)| a != who);
        s.requests.push((who.clone(), e.ts));
        false
    }
}

/// Removes `who` from the open episode's requesters; returns how many are
/// left (0 when no episode is open).
fn remove(s: &mut Session, who: &Option<String>) -> usize {
    s.needs.as_mut().map_or(0, |n| {
        n.waiting.retain(|a| a != who);
        n.waiting.len()
    })
}

/// The `permission_prompt` Notification: the `needs.permission` with no tool.
fn is_prompt(e: &Event) -> bool {
    e.kind == Kind::NeedsPermission && e.detail.tool.is_none()
}
