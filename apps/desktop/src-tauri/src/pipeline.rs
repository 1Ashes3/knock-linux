//! The pipeline (SPEC.md §7.2 steps 3-5). One owner thread holds the engine
//! and everything the app knows about each session; nothing else touches
//! them, so no lock is ever held across an await.
//!
//! In: hook bodies queued by the server, user actions from the commands, and
//! a clock tick every second. The owner parses each body with the
//! `claude-code` adapter, adds the session's process (§9.4, ADR 0014),
//! keeps suppression current (§8.4, §8.6), checks process liveness (§9.7)
//! and steps the engine.
//!
//! Out: the effect runner turns `Notify` and `Escalate` into toasts (§10.2),
//! batches `ReadyBatchAdd` without showing anything (FR-8), writes the
//! snapshot on `Persist`, and publishes `sessions-changed` (§8.5) and the
//! tray count. Each tick it also reads the idle time and sends
//! `away-summary` when the user returns (FR-9, ADR 0034).
//! [`Published`] is what the server and the commands read.

use crate::jump::{Precision, ProcessId};
use crate::platform::{self, Platform};
use crate::settings::{self, Settings};
use knock_core::{
    AWAY_MS, Adapter, Away, AwaySummary, Category, Effect, Engine, Event, Input, Kind, NeedsKind,
    Reason, Tier, iso8601,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

const SNAPSHOT: &str = "sessions.json";
/// The test knock's project name and how long it stays.
const TEST_PROJECT: &str = "Knock test";
const TEST_MS: u64 = 10_000;

/// Testing aid, debug builds only: `KNOCK_AWAY_SECONDS=<n>` (a whole number,
/// 1 or more) makes the user count as away after `n` idle seconds instead
/// of 10 minutes, so FR-9 can be tried by hand without waiting. In
/// PowerShell, before `pnpm dev`: `$env:KNOCK_AWAY_SECONDS = "30"`.
/// Release builds never read it ([`away_threshold`]).
pub const AWAY_VAR: &str = "KNOCK_AWAY_SECONDS";

/// §9.7: 10 minutes, or [`AWAY_VAR`]'s seconds in a debug build. Anything
/// else (unset, empty, 0, not a number, or any release build) is 10 minutes.
pub fn away_threshold(debug: bool, var: Option<&OsStr>) -> u64 {
    let secs = var
        .filter(|_| debug)
        .and_then(|v| v.to_str()?.trim().parse::<u64>().ok());
    secs.filter(|s| *s > 0)
        .map_or(AWAY_MS, |s| s.saturating_mul(1000))
}

pub enum Msg {
    /// A hook request (§8.1), received at `ts` (ms since the Unix epoch), from
    /// `agent`'s route (one of [`knock_core::AGENTS`]).
    /// `owner` is the connection's process (§9.4 step 1), looked up before
    /// the reply; `None` if it wasn't found or `sim` is set.
    Hook {
        ts: u64,
        agent: &'static str,
        body: Vec<u8>,
        sim: bool,
        owner: Option<ProcessId>,
    },
    Ack(String),
    Dismiss(String),
    /// The tray's Pause notifications item: flips the in-effect value.
    TogglePause,
    /// The setup's test knock: a simulated session that needs you, with its
    /// toast even while notifications are suppressed, gone after 10 s.
    TestKnock,
    /// A §8.6 merge patch; `reply` gets the saved settings or why not.
    Settings {
        patch: serde_json::Value,
        reply: Option<oneshot::Sender<Result<Settings, String>>>,
    },
    /// `dismiss_away_summary()` (§8.5): the card folded away.
    DismissAway,
    /// The installed packs were read again (ADR 0038): the active one may
    /// have appeared, changed or gone.
    PacksChanged,
}

/// What the effect runner shows or sends.
#[derive(Debug, PartialEq)]
pub enum Out {
    Toast {
        session: String,
        title: String,
        body: String,
    },
    Sessions(SessionsChanged),
    Settings(Settings),
    /// The Needs-you count, for the tray (§9.7).
    Tray(usize),
    /// `away-summary` (§8.5).
    Away(AwaySummaryView),
}

/// The `sessions-changed` payload (§8.5); also `GET /v1/debug/snapshot`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionsChanged {
    pub rev: u64,
    pub sessions: Vec<SessionView>,
}

/// §8.5 `SessionView`, in the order sessions first appeared.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    /// The agent whose hooks reported it (ADR 0039): `claude-code`, `codex`, `copilot`.
    pub agent: &'static str,
    pub project: String,
    pub tier: &'static str,
    pub state: &'static str,
    pub since: String,
    /// `since` in ms since the Unix epoch, for the strip's own timing
    /// (ADR 0037). Not sent: the UI reads `since`.
    #[serde(skip)]
    pub since_ms: u64,
    pub activity: Option<Category>,
    pub needs: Option<NeedsView>,
    pub precision: Precision,
    pub animal: String,
    pub simulated: bool,
}

/// The `away-summary` payload (§8.5, ADR 0034).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwaySummaryView {
    /// When the idle period began (the last input).
    pub since: String,
    pub items: Vec<AwayItemView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayItemView {
    pub id: String,
    pub project: String,
    /// "ready", "failed" or "needs_you".
    pub outcome: &'static str,
}

impl From<AwaySummary> for AwaySummaryView {
    fn from(s: AwaySummary) -> Self {
        AwaySummaryView {
            since: iso8601(s.since),
            items: (s.items.into_iter())
                .map(|i| AwayItemView {
                    id: i.id,
                    project: i.project,
                    outcome: i.outcome.as_str(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NeedsView {
    pub kind: &'static str,
    pub label: Option<String>,
    pub seen: bool,
}

/// What the server and the commands read; written only by the owner.
#[derive(Default)]
pub struct Published {
    pub sessions: SessionsChanged,
    /// For a jump: each session's process and project.
    pub jump: HashMap<String, (ProcessId, String)>,
    pub settings: Settings,
    pub last_event_at: Option<u64>,
    /// An `away-summary` was sent and not yet dismissed.
    pub away_pending: bool,
}

pub type Shared = Arc<Mutex<Published>>;

pub fn read(s: &Shared) -> MutexGuard<'_, Published> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// A session's process and window, as found by §9.4.
struct Found {
    process: ProcessId,
    /// Executable that owns the window, file name only.
    owner: Option<String>,
    precision: Precision,
}

#[derive(Default)]
struct Run {
    outs: Vec<Out>,
    persist: bool,
}

pub struct Core {
    engine: Engine,
    /// One adapter per agent (ADR 0039), by id.
    adapters: HashMap<&'static str, Box<dyn Adapter + Send>>,
    settings: Settings,
    /// Last value sent as `SetSuppressed`.
    suppressed: bool,
    procs: HashMap<String, Found>,
    /// Sessions whose missing connection owner was already logged.
    unowned: HashSet<String>,
    /// A `Persist` not yet written; [`Core::flush`] writes it.
    dirty: Option<u64>,
    /// FR-8: sessions that became Ready, never shown. FR-9 doesn't read it:
    /// the away summary uses each session's state at return (ADR 0034).
    #[cfg_attr(not(test), allow(dead_code))]
    ready: Vec<String>,
    /// FR-9 idle tracking.
    away: Away,
    /// The last `away-summary` sent, held until `dismiss_away_summary()`;
    /// a new one replaces it.
    away_pending: Option<AwaySummaryView>,
    /// Test-knock sessions and when they end.
    tests: Vec<(String, u64)>,
    view: SessionsChanged,
    needs_you: usize,
    last_event_at: Option<u64>,
    dir: PathBuf,
    platform: Option<Arc<dyn Platform>>,
    shared: Shared,
    /// Installed sprite packs (ADR 0038); `None` in tests: classic only.
    packs: Option<Arc<crate::packs::Packs>>,
    /// Every "connection owner not found" warning, by session: what the
    /// log gets, without depending on `tracing`'s per-thread setup.
    #[cfg(test)]
    owner_warnings: Vec<String>,
}

impl Core {
    /// Gives the core the installed packs, and casts sessions from the
    /// chosen one. Call once, before the pipeline runs.
    pub fn set_packs(&mut self, packs: Arc<crate::packs::Packs>) {
        self.packs = Some(packs);
        let cast = self.cast(&self.settings);
        self.engine.set_settings(self.settings.engine(cast));
    }

    /// The character ids of `settings.pack`, if it is an installed valid
    /// pack.
    fn cast(&self, settings: &Settings) -> Option<Vec<String>> {
        let packs = self.packs.as_ref()?;
        // A hidden built-in pack isn't used, even if still chosen.
        let usable = settings.pack != crate::packs::CLASSIC
            && !settings.hidden_packs.contains(&settings.pack);
        usable.then(|| packs.cast(&settings.pack)).flatten()
    }

    /// Loads the settings from `dir` and tells the engine whether
    /// notifications are suppressed before any event (engine module docs).
    pub fn new(
        dir: PathBuf,
        platform: Option<Arc<dyn Platform>>,
        shared: Shared,
        now: u64,
        local_minute: u32,
    ) -> Self {
        let settings = settings::load(&dir, now);
        let mut engine = Engine::new(settings.engine(None));
        let suppressed = settings.suppressed(local_minute);
        engine.step(Input::SetSuppressed(suppressed));
        let debug = cfg!(debug_assertions);
        let var = debug.then(|| std::env::var_os(AWAY_VAR)).flatten();
        let threshold = away_threshold(debug, var.as_deref());
        if threshold != AWAY_MS {
            tracing::info!(
                seconds = threshold / 1000,
                "away threshold shortened (debug build)"
            );
        }
        let mut core = Core {
            engine,
            adapters: knock_core::AGENTS
                .iter()
                .filter_map(|id| Some((*id, knock_core::adapter(id, users())?)))
                .collect(),
            settings,
            suppressed,
            procs: HashMap::new(),
            unowned: HashSet::new(),
            dirty: None,
            ready: Vec::new(),
            away: Away::new(threshold),
            away_pending: None,
            tests: Vec::new(),
            view: SessionsChanged::default(),
            needs_you: 0,
            last_event_at: None,
            dir,
            platform,
            shared,
            packs: None,
            #[cfg(test)]
            owner_warnings: Vec::new(),
        };
        core.restore_sign_in();
        core.restore_sessions();
        core.publish();
        core
    }

    /// FR-13: brings back the sessions `sessions.json` saved, keeping only
    /// those whose process is still running: the same PID with the same
    /// start time, so a reused PID never brings back a stranger. Without a
    /// platform (tests) or a recorded process nothing is restored; such a
    /// session comes back with its next event.
    fn restore_sessions(&mut self) {
        let Some(p) = self.platform.clone() else {
            return;
        };
        let Ok(bytes) = std::fs::read(self.dir.join(SNAPSHOT)) else {
            return;
        };
        let (saved, found) = saved_sessions(&bytes, |pid| p.process_start(pid));
        if saved.is_empty() {
            return;
        }
        tracing::info!(sessions = saved.len(), "sessions restored");
        self.engine.restore(saved);
        for (id, f) in found {
            self.procs.insert(id, f);
        }
        let now = now_ms();
        self.view = SessionsChanged {
            rev: self.view.rev + 1,
            sessions: self.views(),
        };
        self.needs_you = (self.engine.sessions().iter())
            .filter(|s| s.state.tier() == knock_core::Tier::NeedsYou)
            .count();
        self.persist(now);
    }

    /// ADR 0027: while `launchAtSignIn` is on, a missing or stale sign-in
    /// entry is rewritten at start (an upgrade's uninstall step removes it).
    /// A failure is logged, by kind only (§11.5), and Knock carries on.
    fn restore_sign_in(&self) {
        let Some(p) = &self.platform else { return };
        match p.restore_launch_at_sign_in(self.settings.launch_at_sign_in) {
            Ok(true) => tracing::info!("sign-in entry restored"),
            Ok(false) => {}
            Err(e) => tracing::warn!(kind = ?e.kind(), "sign-in entry not checked"),
        }
    }

    /// Handles one message. Suppression is re-checked first, so an event in
    /// the first second of quiet hours is already covered (§8.4).
    pub fn handle(&mut self, msg: Msg, now: u64, local_minute: u32) -> Vec<Out> {
        let mut run = Run::default();
        self.suppress(local_minute, &mut run);
        match msg {
            Msg::Hook {
                ts,
                agent,
                body,
                sim,
                owner,
            } => self.hook(ts, agent, &body, sim, owner, &mut run),
            Msg::Ack(id) => self.user(Kind::UserAck, &id, now, &mut run),
            Msg::Dismiss(id) => self.user(Kind::UserDismiss, &id, now, &mut run),
            Msg::TogglePause => self.toggle_pause(local_minute, &mut run),
            Msg::TestKnock => self.test_knock(now, &mut run),
            Msg::Settings { patch, reply } => {
                let r = self.update(&patch, local_minute, &mut run);
                if let Some(tx) = reply {
                    let _ = tx.send(r);
                }
            }
            Msg::PacksChanged => {
                let settings = self.settings.clone();
                self.apply_settings(settings, local_minute, &mut run);
            }
            Msg::DismissAway => {
                if self.away_pending.take().is_some() {
                    tracing::info!("away summary dismissed");
                }
            }
        }
        self.finish(run, now)
    }

    /// The once-a-second work: suppression, liveness, then the engine's tick.
    pub fn tick(&mut self, now: u64, local_minute: u32) -> Vec<Out> {
        let mut run = Run::default();
        self.suppress(local_minute, &mut run);
        if let Some(p) = self.platform.clone() {
            let known = self.procs.iter().map(|(id, f)| (id.as_str(), f.process));
            for id in gone(known, |pid| p.process_start(pid)) {
                self.procs.remove(&id);
                run.persist = true;
                let input = Input::ProcessGone {
                    session_id: id,
                    now,
                };
                self.step(input, &mut run);
            }
        }
        let (done, left) = std::mem::take(&mut self.tests)
            .into_iter()
            .partition(|(_, end)| *end <= now);
        self.tests = left;
        for (id, _) in done {
            self.test_hook(now, &id, "SessionEnd", &mut run);
        }
        self.step(Input::Tick { now }, &mut run);
        self.away(now, &mut run);
        self.finish(run, now)
    }

    /// FR-9, after the engine's tick so the states are those at return. An
    /// unreadable idle time skips the tick: away stays as it was.
    fn away(&mut self, now: u64, run: &mut Run) {
        let Some(idle) = self.platform.as_ref().and_then(|p| p.idle_ms()) else {
            return;
        };
        let was = self.away.since();
        let summary = self.away.tick(now, idle, self.engine.sessions());
        if was.is_some() && self.away.since() != was {
            // Item count only (§11.5).
            let items = summary.as_ref().map_or(0, |s| s.items.len());
            tracing::info!(items, "back from away");
        }
        if let Some(s) = summary {
            let view = AwaySummaryView::from(s);
            if self.away_pending.replace(view.clone()).is_some() {
                tracing::info!("away summary replaced");
            }
            run.outs.push(Out::Away(view));
        }
        if self.away.since().is_some_and(|s| was != Some(s)) {
            tracing::info!("away");
        }
    }

    fn hook(
        &mut self,
        ts: u64,
        agent: &str,
        body: &[u8],
        sim: bool,
        owner: Option<ProcessId>,
        run: &mut Run,
    ) {
        self.last_event_at = Some(ts);
        let Some(adapter) = self.adapters.get(agent) else {
            return tracing::warn!("hook for an unknown agent");
        };
        let mut event = match adapter.parse(body, ts) {
            Ok(Some(e)) => e,
            Ok(None) => return,
            // ParseError carries no payload content.
            Err(e) => return tracing::warn!(error = ?e, "hook payload not understood"),
        };
        // Simulated sessions have no process (§8.1).
        if !sim {
            let id = &event.session_id;
            let known = self.procs.get(id).map(|f| f.process);
            // On a session's first request and on every prompt (§9.4, ADR
            // 0014); walked again only when the owner changed.
            if known.is_none() || event.kind == Kind::TurnStarted {
                match owner {
                    // Once per session until it is found, not on every hook.
                    None if known.is_none() => {
                        if self.unowned.insert(id.clone()) {
                            tracing::warn!(session = ?id, "connection owner not found");
                            #[cfg(test)]
                            self.owner_warnings.push(id.clone());
                        }
                    }
                    Some(p) if Some(p) != known => {
                        let found = self.walk(id, p, &event.context.project);
                        self.unowned.remove(id);
                        self.procs.insert(id.clone(), found);
                        run.persist = true;
                    }
                    _ => {}
                }
                // The conversation that just started (or was resumed) is the one
                // live in this window.
                if agent == "agy"
                    && let Some(p) = owner
                {
                    let id = event.session_id.clone();
                    self.end_replaced_agy(&id, p, ts, run);
                }
            }
            event.context.pid = self.procs.get(id).map(|f| f.process.pid);
        }
        self.step(
            Input::Event {
                event,
                simulated: sim,
            },
            run,
        );
    }

    /// agy has no session-end event, and a new conversation (`/new`, or resuming
    /// another) in the same window leaves the old one's sprite behind. A process
    /// has one live agy conversation, so a new one in `process` ends the others
    /// of that process.
    fn end_replaced_agy(&mut self, new: &str, process: ProcessId, ts: u64, run: &mut Run) {
        let old: Vec<String> = (self.procs.iter())
            .filter(|(id, f)| {
                f.process.pid == process.pid && id.starts_with("agy-") && id.as_str() != new
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in old {
            tracing::info!(session = ?id, "agy conversation replaced in its window");
            self.user(Kind::SessionEnded, &id, ts, run);
        }
    }

    /// The user's own test (§10.4 step 5), so its toast shows even when
    /// paused or in quiet hours; only this one bypasses suppression.
    fn test_knock(&mut self, now: u64, run: &mut Run) {
        let id = format!("knock-test-{now}");
        self.test_hook(now, &id, "Notification", run);
        let shown =
            (run.outs.iter()).any(|o| matches!(o, Out::Toast { session, .. } if *session == id));
        let fx = Effect::Notify {
            session_id: id.clone(),
            reason: Reason::NeedsAnswer,
        };
        if let (false, Some((title, body))) =
            (shown, toast_text(&fx, TEST_PROJECT, None, &self.settings))
        {
            tracing::info!(session = ?id, "notify (test knock)");
            run.outs.push(Out::Toast {
                session: id.clone(),
                title,
                body,
            });
        }
        self.tests.push((id, now + TEST_MS));
    }

    /// A simulated hook for a test knock. Not a Claude Code event, so the
    /// health check's last event time stays as it was.
    fn test_hook(&mut self, now: u64, id: &str, hook: &str, run: &mut Run) {
        let body = serde_json::json!({
            "session_id": id,
            "hook_event_name": hook,
            "notification_type": "elicitation_dialog",
            "cwd": TEST_PROJECT,
        });
        let last = self.last_event_at;
        self.hook(
            now,
            "claude-code",
            body.to_string().as_bytes(),
            true,
            None,
            run,
        );
        self.last_event_at = last;
    }

    /// §9.4 step 2 for a new or changed owner.
    fn walk(&self, id: &str, process: ProcessId, project: &str) -> Found {
        let target = self
            .platform
            .as_ref()
            .and_then(|p| p.find_window(process, project));
        tracing::info!(
            session = ?id,
            project = ?project,
            pid = process.pid,
            owner = ?target.as_ref().map(|t| &t.owner),
            precision = ?target.as_ref().map(|t| t.precision),
            "session process"
        );
        Found {
            process,
            owner: target.as_ref().map(|t| t.owner.clone()),
            precision: target.map_or(Precision::None, |t| t.precision),
        }
    }

    fn user(&mut self, kind: Kind, id: &str, now: u64, run: &mut Run) {
        let event = Event::user(kind, id, now);
        let input = Input::Event {
            event,
            simulated: false,
        };
        self.step(input, run);
    }

    fn update(
        &mut self,
        patch: &serde_json::Value,
        local_minute: u32,
        run: &mut Run,
    ) -> Result<Settings, String> {
        let new = settings::merged(&self.settings, patch)?;
        // §9.7: `launchAtSignIn: true/false` changes the sign-in entry first,
        // and the change is undone if the save fails, so the entry and
        // settings.json never disagree. `null` is rejected by `merged`.
        let sign_in = patch
            .get("launchAtSignIn")
            .and_then(serde_json::Value::as_bool);
        let change = match sign_in {
            Some(on) => Some(self.sign_in(on).map_err(|_| {
                "Knock couldn't change whether it starts when you sign in. Please try again."
                    .to_string()
            })?),
            None => None,
        };
        if let Err(e) = settings::save(&self.dir, &new) {
            tracing::warn!(kind = ?e.kind(), "settings not saved");
            // An undone turn-off writes the value back and leaves the Task
            // Manager mark as it was (ADR 0025, 0027).
            let was = self.settings.launch_at_sign_in.unwrap_or(false);
            if let Some(change) = change.filter(|_| sign_in != Some(was)) {
                self.sign_in_ops(change.undo, "launch at sign-in not undone");
            }
            return Err("Knock couldn't save the settings.".into());
        }
        // A turn-off clears the Task Manager mark now. If that fails, the
        // value is already gone, so Knock still doesn't start at sign-in.
        if let Some(change) = change {
            self.sign_in_ops(change.saved, "sign-in mark not cleared");
        }
        self.apply_settings(new.clone(), local_minute, run);
        Ok(new)
    }

    /// Adds or removes the sign-in entry. The error can name the exe's path,
    /// so only its kind is logged (§11.5).
    fn sign_in(&self, on: bool) -> std::io::Result<platform::SignInChange> {
        let p = (self.platform.as_ref()).ok_or_else(|| std::io::Error::other("no platform"))?;
        let r = p.set_launch_at_sign_in(on);
        match &r {
            Ok(_) => tracing::info!(on, "launch at sign-in set"),
            Err(e) => tracing::warn!(on, kind = ?e.kind(), "launch at sign-in not changed"),
        }
        r
    }

    /// Finishes or undoes a sign-in change. A failure is logged by kind
    /// only, under `failed` (§11.5).
    fn sign_in_ops(&self, ops: &[platform::RunOp], failed: &'static str) {
        let Some(p) = self.platform.as_ref().filter(|_| !ops.is_empty()) else {
            return;
        };
        if let Err(e) = p.apply_sign_in_ops(ops) {
            tracing::warn!(kind = ?e.kind(), "{failed}");
        }
    }

    /// From the in-effect value, so two quick clicks toggle twice. Applies
    /// even if the save fails, so the check mark and the behavior agree; the
    /// next successful save persists it.
    fn toggle_pause(&mut self, local_minute: u32, run: &mut Run) {
        let new = Settings {
            pause_notifications: !self.settings.pause_notifications,
            ..self.settings.clone()
        };
        if let Err(e) = settings::save(&self.dir, &new) {
            tracing::warn!(kind = ?e.kind(), "settings not saved; pause applied anyway");
        }
        self.apply_settings(new, local_minute, run);
    }

    /// Applies `new` at once and reports it (`settings-changed`, the tray).
    fn apply_settings(&mut self, new: Settings, local_minute: u32, run: &mut Run) {
        self.engine.set_settings(new.engine(self.cast(&new)));
        self.settings = new;
        self.suppress(local_minute, run);
        run.outs.push(Out::Settings(self.settings.clone()));
    }

    /// Sends `SetSuppressed` when its value changed.
    fn suppress(&mut self, local_minute: u32, run: &mut Run) {
        let on = self.settings.suppressed(local_minute);
        if on != self.suppressed {
            self.suppressed = on;
            tracing::info!(on, "notifications suppressed");
            self.step(Input::SetSuppressed(on), run);
        }
    }

    /// One engine step, then its effects.
    fn step(&mut self, input: Input, run: &mut Run) {
        for fx in self.engine.step(input) {
            match &fx {
                Effect::Persist => run.persist = true,
                Effect::ReadyBatchAdd { session_id } => {
                    self.ready.retain(|s| s != session_id);
                    self.ready.push(session_id.clone());
                }
                Effect::Notify { session_id, .. } | Effect::Escalate { session_id } => {
                    let sessions = self.engine.sessions();
                    let Some(s) = sessions.iter().find(|s| &s.id == session_id) else {
                        continue;
                    };
                    let tool = s.needs.as_ref().and_then(|n| n.label.as_deref());
                    if let Some((title, body)) = toast_text(&fx, &s.project, tool, &self.settings) {
                        tracing::info!(session = ?session_id, effect = ?fx, "notify");
                        run.outs.push(Out::Toast {
                            session: session_id.clone(),
                            title,
                            body,
                        });
                    }
                }
            }
        }
    }

    fn finish(&mut self, run: Run, now: u64) -> Vec<Out> {
        let mut outs = run.outs;
        let sessions = self.engine.sessions();
        self.procs
            .retain(|id, _| sessions.iter().any(|s| &s.id == id));
        self.ready.retain(|id| sessions.iter().any(|s| &s.id == id));
        self.unowned
            .retain(|id| sessions.iter().any(|s| &s.id == id));
        let views = self.views();
        if views != self.view.sessions {
            self.view = SessionsChanged {
                rev: self.view.rev + 1,
                sessions: views,
            };
            outs.push(Out::Sessions(self.view.clone()));
        }
        let needs_you = (self.view.sessions.iter())
            .filter(|v| v.tier == Tier::NeedsYou.as_str())
            .count();
        if needs_you != self.needs_you {
            self.needs_you = needs_you;
            outs.push(Out::Tray(needs_you));
        }
        if run.persist {
            self.dirty = Some(now);
        }
        self.publish();
        outs
    }

    /// Writes the snapshot if a step asked for it. Called after the step's
    /// outputs are applied, so the disk never delays the UI or a toast (§6).
    pub fn flush(&mut self) {
        if let Some(now) = self.dirty.take() {
            self.persist(now);
        }
    }

    fn views(&self) -> Vec<SessionView> {
        let sessions = self.engine.sessions().iter();
        sessions
            .map(|s| SessionView {
                id: s.id.clone(),
                agent: s.agent,
                project: s.project.clone(),
                tier: s.state.tier().as_str(),
                state: s.state.as_str(),
                since: iso8601(s.since),
                since_ms: s.since,
                activity: s.activity,
                needs: s.needs.as_ref().map(|n| NeedsView {
                    kind: needs_kind(n.kind),
                    label: n.label.clone(),
                    seen: n.seen,
                }),
                precision: self
                    .procs
                    .get(&s.id)
                    .map_or(Precision::None, |f| f.precision),
                animal: s.animal.clone(),
                simulated: s.simulated,
            })
            .collect()
    }

    /// `sessions.json`: §11.4 data only. No agent ids (ADR 0019), no cwd
    /// (only its hash), no window titles or handles. Enough to restore from
    /// (FR-13, Phase 5). Simulated sessions (`knock sim`, the test knock) are
    /// left out: none is ever restored.
    fn persist(&self, now: u64) {
        let sessions = (self.engine.sessions().iter())
            .filter(|s| !s.simulated)
            .map(|s| {
                let found = self.procs.get(&s.id);
                serde_json::json!({
                    "id": s.id,
                    "agent": s.agent,
                    "project": s.project,
                    "projectKey": s.project_key,
                    "state": s.state.as_str(),
                    "sinceMs": s.since,
                    "lastEventMs": s.last_event,
                    "activity": s.activity,
                    "needs": s.needs.as_ref().map(|n| serde_json::json!({
                        "kind": needs_kind(n.kind),
                        "label": n.label,
                        "seen": n.seen,
                        "openedMs": n.opened,
                        "escalated": n.escalated,
                    })),
                    "animal": s.animal,
                    "process": found.map(|f| serde_json::json!({
                        "pid": f.process.pid,
                        "start": f.process.start,
                        "owner": f.owner,
                        "precision": f.precision,
                    })),
                })
            })
            .collect::<Vec<_>>();
        let snap = serde_json::json!({ "version": 1, "savedMs": now, "sessions": sessions });
        let bytes = serde_json::to_vec_pretty(&snap).unwrap_or_default();
        if let Err(e) = settings::write_atomic(&self.dir.join(SNAPSHOT), &bytes) {
            tracing::warn!(kind = ?e.kind(), "snapshot not saved");
        }
    }

    fn publish(&self) {
        let mut p = read(&self.shared);
        p.sessions = self.view.clone();
        p.jump = (self.engine.sessions().iter())
            .filter_map(|s| {
                let f = self.procs.get(&s.id)?;
                Some((s.id.clone(), (f.process, s.project.clone())))
            })
            .collect();
        p.settings = self.settings.clone();
        p.last_event_at = self.last_event_at;
        p.away_pending = self.away_pending.is_some();
    }
}

fn needs_kind(k: NeedsKind) -> &'static str {
    match k {
        NeedsKind::Permission => "permission",
        NeedsKind::Answer => "answer",
    }
}

/// §10.2: the toast's title and body for a `Notify` or `Escalate`. `tool`
/// is the episode's label. A permission whose tool is unknown (opened by the
/// `permission_prompt` Notification) leaves out "to use <tool>".
pub fn toast_text(
    fx: &Effect,
    project: &str,
    tool: Option<&str>,
    s: &Settings,
) -> Option<(String, String)> {
    let needs = format!("{project} needs you");
    Some(match fx {
        Effect::Notify { reason, .. } => match reason {
            Reason::NeedsPermission => (
                needs,
                match tool {
                    Some(t) => format!("Claude Code is asking for permission to use {t}."),
                    None => "Claude Code is asking for permission.".into(),
                },
            ),
            Reason::NeedsAnswer => (needs, "Claude Code has a question for you.".into()),
            Reason::Failed => (
                format!("{project} stopped"),
                "Claude Code stopped with an error.".into(),
            ),
            Reason::Stalled => (
                format!("{project} may be stuck"),
                format!("No progress for {}.", minutes(s.stall_minutes)),
            ),
        },
        Effect::Escalate { .. } => (
            format!("{project} is still waiting"),
            format!(
                "Claude Code has been waiting for you for {}.",
                minutes(s.escalation_minutes)
            ),
        ),
        Effect::ReadyBatchAdd { .. } | Effect::Persist => return None,
    })
}

/// "1 minute", "5 minutes" (§10.2 <n> minutes).
fn minutes(n: impl Into<u64>) -> String {
    let n = n.into();
    format!("{n} minute{}", if n == 1 { "" } else { "s" })
}

/// FR-13: the sessions in a `sessions.json`, with their processes, whose
/// process `start_of(pid)` says still runs with the saved start time. A file
/// that isn't version 1, or a session missing a field, is skipped.
fn saved_sessions(
    bytes: &[u8],
    start_of: impl Fn(u32) -> Option<u64>,
) -> (Vec<knock_core::Saved>, Vec<(String, Found)>) {
    use serde_json::Value;
    let Ok(snap) = serde_json::from_slice::<Value>(bytes) else {
        return (Vec::new(), Vec::new());
    };
    if snap["version"] != 1 {
        return (Vec::new(), Vec::new());
    }
    let list = snap["sessions"].as_array().cloned().unwrap_or_default();
    let mut saved = Vec::new();
    let mut found = Vec::new();
    for s in &list {
        let one = || -> Option<(knock_core::Saved, Found)> {
            let text = |v: &Value| v.as_str().map(str::to_string);
            let pr = &s["process"];
            let process = ProcessId {
                pid: u32::try_from(pr["pid"].as_u64()?).ok()?,
                start: pr["start"].as_u64()?,
            };
            if start_of(process.pid) != Some(process.start) {
                return None;
            }
            let needs = match &s["needs"] {
                Value::Null => None,
                n => Some(knock_core::SavedNeeds {
                    kind: knock_core::NeedsKind::parse(n["kind"].as_str()?)?,
                    label: text(&n["label"]),
                    seen: n["seen"].as_bool()?,
                    opened: n["openedMs"].as_u64()?,
                    escalated: n["escalated"].as_bool()?,
                }),
            };
            let precision = match pr["precision"].as_str() {
                Some("exact_window") => Precision::ExactWindow,
                Some("app_window") => Precision::AppWindow,
                _ => Precision::None,
            };
            let session = knock_core::Saved {
                id: text(&s["id"])?,
                agent: text(&s["agent"]).unwrap_or_else(|| "claude-code".into()),
                project: text(&s["project"])?,
                project_key: text(&s["projectKey"])?,
                state: knock_core::State::parse(s["state"].as_str()?)?,
                since: s["sinceMs"].as_u64()?,
                last_event: s["lastEventMs"].as_u64()?,
                activity: s["activity"].as_str().and_then(Category::parse),
                needs,
                animal: text(&s["animal"])?,
            };
            let f = Found {
                process,
                owner: text(&pr["owner"]),
                precision,
            };
            Some((session, f))
        };
        if let Some((session, f)) = one() {
            found.push((session.id.clone(), f));
            saved.push(session);
        }
    }
    (saved, found)
}

/// §9.7 liveness: the sessions whose recorded process no longer runs, or
/// whose PID now belongs to a process with another start time.
pub fn gone<'a>(
    known: impl IntoIterator<Item = (&'a str, ProcessId)>,
    start_of: impl Fn(u32) -> Option<u64>,
) -> Vec<String> {
    let known = known.into_iter();
    known
        .filter(|(_, p)| start_of(p.pid) != Some(p.start))
        .map(|(id, _)| id.to_owned())
        .collect()
}

/// The login name and the last part of the home folder, for the adapter's
/// `home` rule (ADR 0020). Process environment, not an OS API call.
fn users() -> Vec<String> {
    knock_core::paths::user_names(&knock_core::paths::process_env)
}

pub fn now_ms() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH);
    d.map_or(0, |d| d.as_millis() as u64)
}

/// The owner: handles messages as they come and ticks once a second, until
/// every sender is gone. `apply` runs each output.
pub fn run(mut core: Core, rx: Receiver<Msg>, mut apply: impl FnMut(Out)) {
    const SECOND: Duration = Duration::from_secs(1);
    let mut next = Instant::now() + SECOND;
    loop {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(msg) => {
                let outs = core.handle(msg, now_ms(), platform::local_minute());
                outs.into_iter().for_each(&mut apply);
                core.flush();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Checked after every message too, so a busy queue can't hold ticks off.
        let t = Instant::now();
        if t >= next {
            let outs = core.tick(now_ms(), platform::local_minute());
            outs.into_iter().for_each(&mut apply);
            core.flush();
            next = (next + SECOND).max(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::FakePlatform;
    use serde_json::{Value, json};

    #[test]
    fn one_minute_is_singular() {
        assert_eq!(minutes(1u32), "1 minute");
        assert_eq!(minutes(5u32), "5 minutes");
    }

    const T0: u64 = 1_792_166_400_000;
    const NOON: u32 = 12 * 60;
    const MIN: u64 = 60_000;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("knock-pipe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct T {
        core: Core,
        shared: Shared,
        fake: Arc<FakePlatform>,
        dir: PathBuf,
        /// Local minute the hook helpers pass.
        minute: u32,
    }

    impl Drop for T {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn core(name: &str) -> T {
        let dir = temp_dir(name);
        let fake = Arc::new(FakePlatform::default());
        let shared = Shared::default();
        let platform: Arc<dyn Platform> = fake.clone();
        let core = Core::new(dir.clone(), Some(platform), shared.clone(), T0, NOON);
        T {
            core,
            shared,
            fake,
            dir,
            minute: NOON,
        }
    }

    fn body(session: &str, hook: &str, extra: &str) -> Vec<u8> {
        format!(r#"{{"session_id":"{session}","hook_event_name":"{hook}","cwd":"C:\\work\\repo-{session}"{extra}}}"#)
            .into_bytes()
    }

    impl T {
        fn hook(
            &mut self,
            t: u64,
            session: &str,
            hook: &str,
            extra: &str,
            owner: Option<ProcessId>,
        ) -> Vec<Out> {
            let msg = Msg::Hook {
                ts: T0 + t,
                agent: "claude-code",
                body: body(session, hook, extra),
                sim: false,
                owner,
            };
            self.core.handle(msg, T0 + t, self.minute)
        }
        fn sim(&mut self, t: u64, session: &str, hook: &str, extra: &str) -> Vec<Out> {
            let msg = Msg::Hook {
                ts: T0 + t,
                agent: "claude-code",
                body: body(session, hook, extra),
                sim: true,
                owner: None,
            };
            self.core.handle(msg, T0 + t, self.minute)
        }
        fn states(&self) -> Vec<(String, &'static str)> {
            let p = read(&self.shared);
            p.sessions
                .sessions
                .iter()
                .map(|v| (v.id.clone(), v.state))
                .collect()
        }
    }

    fn toasts(outs: &[Out]) -> Vec<(String, String)> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Toast { title, body, .. } => Some((title.clone(), body.clone())),
                _ => None,
            })
            .collect()
    }

    const P1: ProcessId = ProcessId {
        pid: 7596,
        start: 185,
    };
    const P2: ProcessId = ProcessId {
        pid: 5151,
        start: 77,
    };
    const ASK_WRITE: &str = r#","tool_name":"Write","tool_input":{"content":"secret text"}"#;

    #[test]
    fn toast_text_is_exactly_section_10_2() {
        let s = Settings::default();
        let n = |reason| Effect::Notify {
            session_id: "s".into(),
            reason,
        };
        let t = |fx: &Effect, tool| toast_text(fx, "repo", tool, &s).unwrap();
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            t(&n(Reason::NeedsPermission), Some("PowerShell")),
            pair(
                "repo needs you",
                "Claude Code is asking for permission to use PowerShell."
            )
        );
        assert_eq!(
            t(&n(Reason::NeedsPermission), None),
            pair("repo needs you", "Claude Code is asking for permission.")
        );
        assert_eq!(
            t(&n(Reason::NeedsAnswer), None),
            pair("repo needs you", "Claude Code has a question for you.")
        );
        assert_eq!(
            t(
                &Effect::Escalate {
                    session_id: "s".into()
                },
                None
            ),
            pair(
                "repo is still waiting",
                "Claude Code has been waiting for you for 5 minutes."
            )
        );
        assert_eq!(
            t(&n(Reason::Failed), None),
            pair("repo stopped", "Claude Code stopped with an error.")
        );
        assert_eq!(
            t(&n(Reason::Stalled), None),
            pair("repo may be stuck", "No progress for 10 minutes.")
        );
        let s = Settings {
            stall_minutes: 15,
            escalation_minutes: 2,
            ..Settings::default()
        };
        let esc = Effect::Escalate {
            session_id: "s".into(),
        };
        assert!(
            toast_text(&esc, "p", None, &s)
                .unwrap()
                .1
                .contains("for 2 minutes.")
        );
        assert!(
            toast_text(&n(Reason::Stalled), "p", None, &s)
                .unwrap()
                .1
                .contains("for 15 minutes.")
        );
        let ready = Effect::ReadyBatchAdd {
            session_id: "s".into(),
        };
        assert_eq!(toast_text(&ready, "p", None, &s), None);
        assert_eq!(toast_text(&Effect::Persist, "p", None, &s), None);
    }

    #[test]
    fn session_view_uses_the_exact_spec_field_names() {
        let v = SessionView {
            id: "s1".into(),
            agent: "claude-code",
            project: "repo-name".into(),
            tier: "needs_you",
            state: "needs_input",
            since: iso8601(T0),
            since_ms: T0,
            activity: Some(Category::Run),
            needs: Some(NeedsView {
                kind: "permission",
                label: Some("PowerShell".into()),
                seen: false,
            }),
            precision: Precision::AppWindow,
            animal: "fox".into(),
            simulated: false,
        };
        let payload = SessionsChanged {
            rev: 3,
            sessions: vec![v.clone()],
        };
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            concat!(
                r#"{"rev":3,"sessions":[{"id":"s1","agent":"claude-code","project":"repo-name","tier":"needs_you","#,
                r#""state":"needs_input","since":"2026-10-16T16:00:00.000Z","activity":"run","#,
                r#""needs":{"kind":"permission","label":"PowerShell","seen":false},"#,
                r#""precision":"app_window","animal":"fox","simulated":false}]}"#
            )
        );
        let none = SessionView {
            activity: None,
            needs: None,
            ..v
        };
        let json = serde_json::to_value(&none).unwrap();
        assert_eq!(
            (json["activity"].clone(), json["needs"].clone()),
            (Value::Null, Value::Null)
        );
    }

    #[test]
    fn away_summary_uses_the_exact_spec_shape() {
        let s = knock_core::AwaySummary {
            since: T0,
            items: vec![
                knock_core::AwayItem {
                    id: "s1".into(),
                    project: "web-shop".into(),
                    outcome: knock_core::Outcome::Ready,
                },
                knock_core::AwayItem {
                    id: "s2".into(),
                    project: "api".into(),
                    outcome: knock_core::Outcome::Failed,
                },
                knock_core::AwayItem {
                    id: "s3".into(),
                    project: "jobs".into(),
                    outcome: knock_core::Outcome::NeedsYou,
                },
            ],
        };
        assert_eq!(
            serde_json::to_string(&AwaySummaryView::from(s)).unwrap(),
            concat!(
                r#"{"since":"2026-10-16T16:00:00.000Z","items":["#,
                r#"{"id":"s1","project":"web-shop","outcome":"ready"},"#,
                r#"{"id":"s2","project":"api","outcome":"failed"},"#,
                r#"{"id":"s3","project":"jobs","outcome":"needs_you"}]}"#
            )
        );
    }

    #[test]
    fn the_away_threshold_is_shortened_only_in_a_debug_build() {
        let os = |s: &'static str| Some(OsStr::new(s));
        assert_eq!(away_threshold(true, None), 10 * MIN);
        assert_eq!(away_threshold(true, os("30")), 30_000);
        assert_eq!(away_threshold(true, os(" 5 ")), 5_000);
        for bad in ["", "0", "-5", "1.5", "abc", "30s"] {
            assert_eq!(away_threshold(true, os(bad)), 10 * MIN, "{bad:?}");
        }
        for any in [None, os("30"), os("1")] {
            assert_eq!(away_threshold(false, any), 10 * MIN, "release: {any:?}");
        }
    }

    fn aways(outs: &[Out]) -> Vec<AwaySummaryView> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Away(a) => Some(a.clone()),
                _ => None,
            })
            .collect()
    }

    impl T {
        /// One tick at `t` with the user idle since `input`.
        fn idle_tick(&mut self, t: u64, input: u64) -> Vec<Out> {
            *self.fake.idle.lock().unwrap() = Some(t - input);
            self.core.tick(T0 + t, self.minute)
        }
    }

    /// FR-9 end to end through the pipeline: the idle time comes from the
    /// platform, a summary on return, held until dismissed, replaced by the
    /// next one, and nothing when the idle time can't be read.
    #[test]
    fn away_summary_on_return_held_until_dismissed_and_replaced() {
        let mut t = core("away");
        t.hook(0, "a", "UserPromptSubmit", "", None);
        t.sim(0, "b", "UserPromptSubmit", "");
        // Last input at 1 s; away at 10 min 1 s.
        t.hook(5 * MIN, "a", "Stop", "", None);
        t.sim(6 * MIN, "b", "PermissionRequest", ASK_WRITE);
        assert!(aways(&t.idle_tick(10 * MIN, 1000)).is_empty());
        assert_eq!(t.core.away.since(), None, "9:59");
        assert!(aways(&t.idle_tick(1000 + 10 * MIN, 1000)).is_empty());
        assert_eq!(t.core.away.since(), Some(T0 + 1000));
        assert!(aways(&t.idle_tick(12 * MIN, 1000)).is_empty());
        // Unreadable idle time: no change.
        *t.fake.idle.lock().unwrap() = None;
        assert!(aways(&t.core.tick(T0 + 13 * MIN, NOON)).is_empty());
        let outs = t.idle_tick(14 * MIN, 14 * MIN - 200);
        let got = aways(&outs);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].since, iso8601(T0 + 1000));
        let items: Vec<_> = (got[0].items.iter())
            .map(|i| (i.id.as_str(), i.project.as_str(), i.outcome))
            .collect();
        assert_eq!(
            items,
            [("a", "repo-a", "ready"), ("b", "repo-b", "needs_you")]
        );
        assert!(read(&t.shared).away_pending);
        assert_eq!(t.core.away_pending.as_ref(), Some(&got[0]));
        // No new summary while the user is back.
        assert!(aways(&t.idle_tick(15 * MIN, 14 * MIN)).is_empty());

        // A second idle period: its summary replaces the first.
        t.hook(16 * MIN, "a", "UserPromptSubmit", "", None);
        t.hook(
            17 * MIN,
            "a",
            "StopFailure",
            r#","error":"rate_limit""#,
            None,
        );
        t.idle_tick(15 * MIN + 10 * MIN, 15 * MIN);
        let got2 = aways(&t.idle_tick(26 * MIN, 26 * MIN));
        assert_eq!(got2.len(), 1);
        assert_eq!(got2[0].since, iso8601(T0 + 15 * MIN));
        assert_eq!(got2[0].items.len(), 1);
        assert_eq!(got2[0].items[0].outcome, "failed");
        assert_eq!(t.core.away_pending.as_ref(), Some(&got2[0]));

        // dismiss_away_summary() clears it and sends nothing.
        let outs = t.core.handle(Msg::DismissAway, T0 + 26 * MIN, NOON);
        assert!(outs.is_empty(), "{outs:?}");
        assert_eq!(t.core.away_pending, None);
        assert!(!read(&t.shared).away_pending);
        // Dismissing again is harmless.
        assert!(
            t.core
                .handle(Msg::DismissAway, T0 + 26 * MIN, NOON)
                .is_empty()
        );
    }

    #[test]
    fn no_away_summary_when_nothing_happened_while_away() {
        let mut t = core("away-empty");
        t.hook(0, "a", "Stop", "", None);
        t.idle_tick(1000 + 10 * MIN, 1000);
        assert!(aways(&t.idle_tick(20 * MIN, 20 * MIN)).is_empty());
        assert_eq!(t.core.away_pending, None);
        assert!(!read(&t.shared).away_pending);
    }

    #[test]
    fn a_permission_knocks_once_with_the_spec_text_and_free_text_never_leaves() {
        let mut t = core("permission");
        let outs = t.hook(0, "a", "PermissionRequest", ASK_WRITE, Some(P1));
        assert_eq!(
            toasts(&outs),
            [(
                "repo-a needs you".into(),
                "Claude Code is asking for permission to use Write.".into()
            )]
        );
        assert!(outs.contains(&Out::Tray(1)));
        // The late `permission_prompt` Notification is the same episode.
        let outs = t.hook(
            6000,
            "a",
            "Notification",
            r#","notification_type":"permission_prompt","message":"secret text""#,
            Some(P1),
        );
        assert!(toasts(&outs).is_empty());
        // Nothing anywhere carries the free text.
        let mut seen = format!("{outs:?}");
        seen += &serde_json::to_string(&read(&t.shared).sessions).unwrap();
        t.core.flush();
        seen += &std::fs::read_to_string(t.dir.join(SNAPSHOT)).unwrap();
        assert!(!seen.contains("secret"), "{seen}");
    }

    #[test]
    fn ready_never_shows_anything_and_is_batched() {
        let mut t = core("ready");
        t.hook(
            0,
            "a",
            "UserPromptSubmit",
            r#","prompt":"secret""#,
            Some(P1),
        );
        let outs = t.hook(1000, "a", "Stop", "", Some(P1));
        assert!(toasts(&outs).is_empty(), "FR-8");
        assert_eq!(t.core.ready, ["a"]);
        assert_eq!(t.states(), [("a".into(), "ready")]);
    }

    #[test]
    fn failed_and_escalation_toasts_and_no_escalation_once_seen() {
        let mut t = core("escalate");
        t.hook(0, "a", "PermissionRequest", ASK_WRITE, Some(P1));
        t.hook(0, "b", "PermissionRequest", ASK_WRITE, None);
        t.core.handle(Msg::Ack("b".into()), T0 + 1000, NOON);
        *t.fake.alive.lock().unwrap() = vec![P1];
        let outs = t.core.tick(T0 + 5 * MIN, NOON);
        assert_eq!(
            toasts(&outs),
            [(
                "repo-a is still waiting".into(),
                "Claude Code has been waiting for you for 5 minutes.".into()
            )]
        );
        let outs = t.hook(
            5 * MIN + 1,
            "a",
            "StopFailure",
            r#","error":"rate_limit""#,
            Some(P1),
        );
        assert_eq!(
            toasts(&outs),
            [(
                "repo-a stopped".into(),
                "Claude Code stopped with an error.".into()
            )]
        );
    }

    #[test]
    fn pause_and_quiet_hours_suppress_toasts_and_take_effect_at_once() {
        let mut t = core("suppress");
        let patch = |v: Value| Msg::Settings {
            patch: v,
            reply: None,
        };
        let outs = t
            .core
            .handle(patch(json!({ "pauseNotifications": true })), T0, NOON);
        assert!(matches!(&outs[..], [Out::Settings(s)] if s.pause_notifications));
        let outs = t.hook(0, "a", "PermissionRequest", ASK_WRITE, None);
        assert!(toasts(&outs).is_empty());
        assert_eq!(
            t.states(),
            [("a".into(), "needs_input")],
            "the strip still shows it"
        );
        // Saved, and it holds after a restart.
        let saved = settings::load(&t.dir, T0);
        assert!(saved.pause_notifications);

        // Quiet hours 22:00-08:00, across midnight, checked on every tick.
        t.core.handle(
            patch(json!({ "pauseNotifications": false, "quietHours": { "enabled": true } })),
            T0,
            NOON,
        );
        t.minute = 21 * 60 + 59;
        t.core.tick(T0 + 1000, t.minute);
        let outs = t.hook(2000, "b", "PermissionRequest", ASK_WRITE, None);
        assert_eq!(toasts(&outs).len(), 1, "21:59 is not quiet");
        t.minute = 22 * 60;
        t.core.tick(T0 + 3000, t.minute);
        let outs = t.hook(4000, "c", "PermissionRequest", ASK_WRITE, None);
        assert!(toasts(&outs).is_empty(), "22:00 is quiet");
        t.minute = 3 * 60;
        t.core.tick(T0 + 5000, t.minute);
        assert!(toasts(&t.hook(6000, "d", "PermissionRequest", ASK_WRITE, None)).is_empty());
        t.minute = 8 * 60;
        t.core.tick(T0 + 7000, t.minute);
        assert_eq!(
            toasts(&t.hook(8000, "e", "PermissionRequest", ASK_WRITE, None)).len(),
            1
        );
    }

    #[test]
    fn quiet_hours_cover_a_hook_before_the_next_tick() {
        let mut t = core("quiet-hook");
        let msg = Msg::Settings {
            patch: json!({ "quietHours": { "enabled": true } }),
            reply: None,
        };
        t.core.handle(msg, T0, NOON);
        t.minute = 21 * 60 + 59;
        t.core.tick(T0 + 1000, t.minute);
        // 22:00 starts between ticks: the hook itself checks.
        t.minute = 22 * 60;
        let outs = t.hook(1500, "a", "PermissionRequest", ASK_WRITE, None);
        assert!(toasts(&outs).is_empty(), "first second of quiet hours");
        // 08:00 ends it the same way.
        t.minute = 8 * 60;
        let outs = t.hook(2000, "b", "PermissionRequest", ASK_WRITE, None);
        assert_eq!(toasts(&outs).len(), 1);
    }

    fn pauses(outs: &[Out]) -> Vec<bool> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Settings(s) => Some(s.pause_notifications),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tray_pause_toggles_from_the_in_effect_value() {
        let mut t = core("toggle");
        // Two quick clicks, both queued before either is handled: on, then off.
        assert_eq!(pauses(&t.core.handle(Msg::TogglePause, T0, NOON)), [true]);
        assert_eq!(pauses(&t.core.handle(Msg::TogglePause, T0, NOON)), [false]);
        assert!(!settings::load(&t.dir, T0).pause_notifications);
        assert!(!read(&t.shared).settings.pause_notifications);
        t.core.handle(Msg::TogglePause, T0, NOON);
        assert!(settings::load(&t.dir, T0).pause_notifications, "saved");
        assert!(toasts(&t.hook(0, "a", "PermissionRequest", ASK_WRITE, None)).is_empty());
    }

    #[test]
    fn tray_pause_applies_even_when_the_save_fails() {
        let mut t = core("toggle-fail");
        // A folder where the temporary file goes makes every save fail.
        let block = t.dir.join("settings.json.tmp");
        std::fs::create_dir(&block).unwrap();
        let outs = t.core.handle(Msg::TogglePause, T0, NOON);
        assert_eq!(
            pauses(&outs),
            [true],
            "the check mark gets the in-effect value"
        );
        assert!(read(&t.shared).settings.pause_notifications);
        assert!(!settings::load(&t.dir, T0).pause_notifications, "not saved");
        let outs = t.hook(0, "a", "PermissionRequest", ASK_WRITE, None);
        assert!(toasts(&outs).is_empty(), "suppressed all the same");
        assert_eq!(t.states(), [("a".into(), "needs_input")]);
        // The next successful save persists it.
        std::fs::remove_dir(&block).unwrap();
        let msg = Msg::Settings {
            patch: json!({ "sounds": true }),
            reply: None,
        };
        t.core.handle(msg, T0, NOON);
        let saved = settings::load(&t.dir, T0);
        assert!(saved.pause_notifications && saved.sounds);
    }

    #[test]
    fn the_snapshot_is_written_after_the_outputs() {
        let mut t = core("flush");
        let outs = t.hook(0, "a", "PermissionRequest", ASK_WRITE, Some(P1));
        assert_eq!(toasts(&outs).len(), 1);
        assert!(outs.iter().any(|o| matches!(o, Out::Sessions(_))));
        assert!(!t.dir.join(SNAPSHOT).exists(), "not before the outputs");
        t.core.flush();
        let text = std::fs::read_to_string(t.dir.join(SNAPSHOT)).unwrap();
        assert!(text.contains("needs_input"));
        assert!(
            !t.dir.join("sessions.json.tmp").exists(),
            "atomic: no leftover"
        );
        // Nothing new: no write.
        std::fs::remove_file(t.dir.join(SNAPSHOT)).unwrap();
        t.core.flush();
        assert!(!t.dir.join(SNAPSHOT).exists());
    }

    #[test]
    fn a_missing_owner_is_logged_once_per_session_until_found() {
        // Counted where the warning is logged: checking the log file through
        // a test-only `tracing` subscriber was flaky under parallel tests.
        let mut t = core("owner-log");
        for i in 0..5 {
            t.hook(i, "a", "PreToolUse", r#","tool_name":"Read""#, None);
        }
        t.hook(10, "b", "Stop", "", None);
        t.hook(20, "a", "UserPromptSubmit", "", Some(P1));
        t.hook(30, "a", "Stop", "", None);
        assert_eq!(t.core.owner_warnings, ["a", "b"]);
    }

    /// agy has no session-end event: a new conversation in the same window ends the
    /// old one, while another window's conversation (another process) stays.
    #[test]
    fn a_new_agy_conversation_replaces_the_old_one_in_its_window() {
        let mut t = core("agy-replaced");
        let agy = |t: &mut T, ts: u64, id: &str, owner: ProcessId| {
            let body = format!(
                r#"{{"conversationId":"{id}","workspacePaths":["/w/repo"],"hook_event_name":"PreInvocation","invocationNum":0}}"#
            );
            let msg = Msg::Hook {
                ts: T0 + ts,
                agent: "agy",
                body: body.into_bytes(),
                sim: false,
                owner: Some(owner),
            };
            t.core.handle(msg, T0 + ts, t.minute);
        };
        let ids = |t: &T| {
            let mut v: Vec<String> = t
                .core
                .views()
                .into_iter()
                .filter(|s| s.state != "ended")
                .map(|s| s.id)
                .collect();
            v.sort();
            v
        };
        agy(&mut t, 1, "one", P1);
        agy(&mut t, 2, "other-window", P2);
        assert_eq!(ids(&t), ["agy-one", "agy-other-window"]);
        agy(&mut t, 3, "two", P1);
        assert_eq!(ids(&t), ["agy-other-window", "agy-two"]);
        // Going back to the first conversation (resuming it) makes that the live one.
        agy(&mut t, 4, "one", P1);
        assert_eq!(ids(&t), ["agy-one", "agy-other-window"]);
    }

    /// FR-13: after a restart, sessions whose process still runs come back
    /// with their process (so a click still jumps); the rest don't.
    #[test]
    fn a_restart_restores_sessions_whose_process_still_runs() {
        let mut t = core("restore");
        t.hook(1, "a", "UserPromptSubmit", "", Some(P1));
        t.hook(
            2,
            "a",
            "PermissionRequest",
            r#","tool_name":"Bash""#,
            Some(P1),
        );
        t.hook(3, "b", "UserPromptSubmit", "", Some(P2));
        t.core.flush();
        let restart = |alive: Vec<ProcessId>| {
            let fake = Arc::new(FakePlatform::default());
            *fake.alive.lock().unwrap() = alive;
            let platform: Arc<dyn Platform> = fake;
            Core::new(t.dir.clone(), Some(platform), Shared::default(), T0, NOON)
        };
        // Only a's process still runs.
        let c = restart(vec![P1]);
        let ids: Vec<_> = c
            .engine
            .sessions()
            .iter()
            .map(|s| (s.id.as_str(), s.state))
            .collect();
        assert_eq!(ids, [("a", knock_core::State::NeedsInput)]);
        assert_eq!(c.procs["a"].process, P1);
        assert_eq!(c.view.sessions.len(), 1, "published");
        assert_eq!(c.needs_you, 1);
        // A reused PID (same number, another start time) brings back nothing.
        let reused = ProcessId {
            pid: P1.pid,
            start: P1.start + 1,
        };
        assert!(restart(vec![reused]).engine.sessions().is_empty());
        // Nothing alive, or no platform: nothing restored.
        assert!(restart(vec![]).engine.sessions().is_empty());
        let none = Core::new(t.dir.clone(), None, Shared::default(), T0, NOON);
        assert!(none.engine.sessions().is_empty());
    }

    #[test]
    fn a_damaged_or_old_snapshot_restores_nothing() {
        let alive = |_| Some(1);
        for bytes in [
            &b"{"[..],
            br#"{"version":2,"sessions":[]}"#,
            br#"{"version":1}"#,
        ] {
            assert!(saved_sessions(bytes, alive).0.is_empty());
        }
        // A session missing a field is skipped; the others still come back.
        let snap = br#"{"version":1,"sessions":[
            {"id":"x","project":"p","projectKey":"k","state":"working","sinceMs":1,"lastEventMs":1,"animal":"fox","needs":null,"process":{"pid":7,"start":1}},
            {"id":"y","state":"working","process":{"pid":7,"start":1}}
        ]}"#;
        let (saved, found) = saved_sessions(snap, alive);
        assert_eq!(saved.len(), 1);
        assert_eq!(found[0].1.precision, Precision::None);
    }

    #[test]
    fn update_settings_replies_and_rejects_a_bad_patch() {
        let mut t = core("update");
        let (tx, mut rx) = oneshot::channel();
        let msg = Msg::Settings {
            patch: json!({ "size": "huge" }),
            reply: Some(tx),
        };
        let outs = t.core.handle(msg, T0, NOON);
        assert!(rx.try_recv().unwrap().is_err());
        assert!(outs.is_empty());
        let (tx, mut rx) = oneshot::channel();
        let msg = Msg::Settings {
            patch: json!({ "size": "large", "stallMinutes": 3 }),
            reply: Some(tx),
        };
        t.core.handle(msg, T0, NOON);
        let s = rx.try_recv().unwrap().unwrap();
        assert_eq!((s.size, s.stall_minutes), (settings::Size::Large, 3));
        assert_eq!(read(&t.shared).settings, s);
        // The engine got it: stalls after 3 minutes now.
        t.hook(0, "a", "UserPromptSubmit", "", None);
        t.core.tick(T0 + 3 * MIN, NOON);
        assert_eq!(t.states(), [("a".into(), "stalled")]);
    }

    #[test]
    fn the_sign_in_entry_and_the_saved_settings_never_disagree() {
        let mut t = core("sign-in");
        let set = |t: &mut T, patch: Value| {
            let (tx, mut rx) = oneshot::channel();
            let msg = Msg::Settings {
                patch,
                reply: Some(tx),
            };
            t.core.handle(msg, T0, NOON);
            rx.try_recv().unwrap()
        };
        let calls = |t: &T| t.fake.sign_in.lock().unwrap().clone();
        let saved = |t: &T| settings::load(&t.dir, T0).launch_at_sign_in;
        // The entry first, then the save.
        let s = set(&mut t, json!({ "launchAtSignIn": true })).unwrap();
        assert_eq!(s.launch_at_sign_in, Some(true));
        assert_eq!((calls(&t), saved(&t)), (vec![true], Some(true)));
        // An invalid patch, or `null`, never touches the entry.
        assert!(set(&mut t, json!({ "launchAtSignIn": false, "size": "huge" })).is_err());
        let e = set(&mut t, json!({ "launchAtSignIn": null })).unwrap_err();
        assert_eq!(e, "Start when you sign in must be on or off.");
        assert_eq!(saved(&t), Some(true));
        set(&mut t, json!({ "launchAtSignIn": true })).unwrap();
        assert_eq!(calls(&t), [true, true]);
        // The entry can't be changed: nothing is saved, the error is shown.
        *t.fake.sign_in_fails.lock().unwrap() = true;
        let e = set(&mut t, json!({ "launchAtSignIn": false, "sounds": true })).unwrap_err();
        assert_eq!(
            e,
            "Knock couldn't change whether it starts when you sign in. Please try again."
        );
        assert_eq!(saved(&t), Some(true));
        assert!(!settings::load(&t.dir, T0).sounds);
        assert_eq!(read(&t.shared).settings.launch_at_sign_in, Some(true));
        *t.fake.sign_in_fails.lock().unwrap() = false;
        // The user disabled Knock in Task Manager, then turns it off in
        // Knock, and the save fails: the value is written back and the mark
        // is left as it was, so Knock doesn't start at sign-in.
        let disabled = vec![3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
        *t.fake.approved.lock().unwrap() = Some(disabled.clone());
        let entry = |t: &T| {
            let run = t.fake.run.lock().unwrap().clone();
            (run, t.fake.approved.lock().unwrap().clone())
        };
        let block = t.dir.join("settings.json.tmp");
        std::fs::create_dir(&block).unwrap();
        let e = set(&mut t, json!({ "launchAtSignIn": false })).unwrap_err();
        assert_eq!(e, "Knock couldn't save the settings.");
        assert_eq!(calls(&t), [true, true, false, false]);
        let own = Some(platform::FAKE_EXE.to_string());
        assert_eq!(entry(&t), (own.clone(), Some(disabled.clone())), "undone");
        assert_eq!(saved(&t), Some(true));
        assert_eq!(read(&t.shared).settings.launch_at_sign_in, Some(true));
        // Same value again: nothing to undo (turning on clears the mark).
        assert!(set(&mut t, json!({ "launchAtSignIn": true })).is_err());
        assert_eq!(calls(&t).len(), 5);
        assert_eq!(entry(&t), (own.clone(), None));
        // A saved turn-off removes the value and clears the mark.
        std::fs::remove_dir(&block).unwrap();
        *t.fake.approved.lock().unwrap() = Some(disabled.clone());
        set(&mut t, json!({ "launchAtSignIn": false })).unwrap();
        assert_eq!((entry(&t), saved(&t)), ((None, None), Some(false)));
        // An undone turn-on deletes the value again.
        std::fs::create_dir(&block).unwrap();
        assert!(set(&mut t, json!({ "launchAtSignIn": true })).is_err());
        assert_eq!((entry(&t), saved(&t)), ((None, None), Some(false)));
        std::fs::remove_dir(&block).unwrap();
    }

    /// ADR 0027: at start, only while on, and a failure doesn't stop Knock.
    #[test]
    fn at_start_the_sign_in_entry_is_restored_only_while_on() {
        let start = |name: &str, setting: Option<bool>, fails: bool| {
            let dir = temp_dir(name);
            let s = Settings {
                launch_at_sign_in: setting,
                ..Settings::default()
            };
            settings::save(&dir, &s).unwrap();
            let fake = Arc::new(FakePlatform::default());
            *fake.sign_in_fails.lock().unwrap() = fails;
            let platform: Arc<dyn Platform> = fake.clone();
            let core = Core::new(dir.clone(), Some(platform), Shared::default(), T0, NOON);
            assert_eq!(core.settings.launch_at_sign_in, setting);
            std::fs::remove_dir_all(&dir).unwrap();
            fake.run.lock().unwrap().clone()
        };
        let own = Some(platform::FAKE_EXE.to_string());
        assert_eq!(start("start-on", Some(true), false), own);
        assert_eq!(start("start-off", Some(false), false), None);
        assert_eq!(start("start-unset", None, false), None);
        assert_eq!(start("start-fails", Some(true), true), None, "still starts");
    }

    #[test]
    fn simulated_sessions_and_the_test_knock_are_never_saved() {
        let mut t = core("sim-snapshot");
        t.hook(0, "real", "Stop", "", Some(P1));
        t.sim(0, "simulated", "Stop", "");
        t.core.handle(Msg::TestKnock, T0 + 1, NOON);
        assert_eq!(read(&t.shared).sessions.sessions.len(), 3, "all shown");
        t.core.flush();
        let text = std::fs::read_to_string(t.dir.join(SNAPSHOT)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let ids: Vec<_> = (v["sessions"].as_array().unwrap().iter())
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["real"]);
        assert!(!text.contains("knock-test") && !text.contains("Knock test"));
    }

    #[test]
    fn rev_goes_up_by_one_per_change_and_only_on_change() {
        let mut t = core("rev");
        let revs = |outs: &[Out]| -> Vec<u64> {
            outs.iter()
                .filter_map(|o| match o {
                    Out::Sessions(p) => Some(p.rev),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(revs(&t.hook(0, "a", "UserPromptSubmit", "", None)), [1]);
        assert_eq!(
            revs(&t.hook(10, "a", "PreToolUse", r#","tool_name":"Read""#, None)),
            [2],
            "activity changed"
        );
        assert_eq!(
            revs(&t.hook(20, "a", "PostToolUse", r#","tool_name":"Read""#, None)),
            Vec::<u64>::new()
        );
        assert_eq!(revs(&t.core.tick(T0 + 1000, NOON)), Vec::<u64>::new());
        assert_eq!(revs(&t.sim(30, "b", "Stop", "")), [3]);
        assert_eq!(read(&t.shared).sessions.rev, 3);
        // In first-appearance order, the sim one marked.
        let p = read(&t.shared);
        let ids: Vec<_> = p
            .sessions
            .sessions
            .iter()
            .map(|v| (v.id.as_str(), v.simulated))
            .collect();
        assert_eq!(ids, [("a", false), ("b", true)]);
    }

    #[test]
    fn a_known_process_fills_pid_and_keeps_the_session_past_six_hours() {
        let mut t = core("pid");
        t.hook(0, "known", "Stop", "", Some(P1));
        t.hook(0, "unknown", "Stop", "", None);
        t.sim(0, "sim", "Stop", "");
        *t.fake.alive.lock().unwrap() = vec![P1];
        t.core.tick(T0 + 6 * 60 * MIN, NOON);
        // No pid ever: removed 6 h after the last event (ADR 0018 item 7).
        assert_eq!(t.states(), [("known".into(), "ready")]);
        let p = read(&t.shared);
        assert_eq!(p.jump["known"], (P1, "repo-known".to_string()));
        assert_eq!(p.sessions.sessions[0].precision, Precision::AppWindow);
    }

    #[test]
    fn a_gone_process_ends_the_session_and_sim_sessions_are_never_checked() {
        let mut t = core("gone");
        t.hook(0, "a", "UserPromptSubmit", "", Some(P1));
        t.sim(0, "s", "UserPromptSubmit", "");
        *t.fake.alive.lock().unwrap() = vec![P1];
        t.core.tick(T0 + 1000, NOON);
        assert_eq!(
            t.states(),
            [("a".into(), "working"), ("s".into(), "working")]
        );
        // PID reused by a newer process: gone.
        *t.fake.alive.lock().unwrap() = vec![ProcessId {
            pid: P1.pid,
            start: 999,
        }];
        t.core.tick(T0 + 2000, NOON);
        assert_eq!(
            t.states(),
            [("a".into(), "ended_unexpectedly"), ("s".into(), "working")]
        );
        assert!(read(&t.shared).jump.is_empty());
        // `--resume` from a new process: looked up and walked again.
        let p2 = ProcessId {
            pid: 8000,
            start: 300,
        };
        t.hook(3000, "a", "PreToolUse", r#","tool_name":"Read""#, Some(p2));
        assert_eq!(t.states()[0], ("a".into(), "working"));
        assert_eq!(*t.fake.walks.lock().unwrap(), [P1.pid, p2.pid]);
    }

    #[test]
    fn owner_is_replaced_only_on_a_prompt() {
        let mut t = core("owner");
        let p2 = ProcessId {
            pid: 8000,
            start: 300,
        };
        t.hook(0, "a", "PreToolUse", r#","tool_name":"Read""#, Some(P1));
        t.hook(10, "a", "PreToolUse", r#","tool_name":"Read""#, Some(p2));
        assert_eq!(read(&t.shared).jump["a"].0, P1, "not a prompt: kept");
        t.hook(20, "a", "UserPromptSubmit", "", Some(P1));
        assert_eq!(
            t.fake.walks.lock().unwrap().len(),
            1,
            "same owner: not walked"
        );
        t.hook(30, "a", "UserPromptSubmit", "", Some(p2));
        assert_eq!(read(&t.shared).jump["a"].0, p2);
        t.hook(40, "a", "UserPromptSubmit", "", None);
        assert_eq!(read(&t.shared).jump["a"].0, p2, "a failed lookup keeps it");
    }

    #[test]
    fn snapshot_holds_no_agent_id_cwd_or_free_text() {
        let mut t = core("snapshot");
        let cwd = r#","cwd":"C:\\Users\\someone\\secret-folder\\repo""#;
        let sub =
            r#","agent_id":"agentsecret123","tool_name":"Bash","tool_input":{"command":"secret"}"#;
        let msg = Msg::Hook {
            ts: T0,
            agent: "claude-code",
            body: format!(
                r#"{{"session_id":"a","hook_event_name":"PermissionRequest"{cwd}{sub}}}"#
            )
            .into_bytes(),
            sim: false,
            owner: Some(P1),
        };
        t.core.handle(msg, T0, NOON);
        t.core.flush();
        let text = std::fs::read_to_string(t.dir.join(SNAPSHOT)).unwrap();
        for bad in [
            "agentsecret123",
            "secret-folder",
            "someone",
            "Users",
            "C:\\",
            "secret",
        ] {
            assert!(!text.contains(bad), "{bad} in {text}");
        }
        let v: Value = serde_json::from_str(&text).unwrap();
        let s = &v["sessions"][0];
        assert_eq!(s["project"], "repo");
        assert_eq!(
            s["projectKey"],
            knock_core::project_key(r"C:\Users\someone\secret-folder\repo")
        );
        assert_eq!(s["state"], "needs_input");
        assert_eq!(s["agent"], "claude-code");
        assert_eq!(s["needs"]["label"], "Bash");
        assert_eq!(s["process"]["pid"], P1.pid);
        assert_eq!(s["process"]["owner"], "WindowsTerminal.exe");
        let mut keys: Vec<_> = s.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "activity",
                "agent",
                "animal",
                "id",
                "lastEventMs",
                "needs",
                "process",
                "project",
                "projectKey",
                "sinceMs",
                "state"
            ]
        );
    }

    // Liveness on the process tables recorded for ADR 0012 (jump.rs tests):
    // claude.exe 7596 (start 185) in a classic console, 9116 (1500) in
    // Windows Terminal.
    #[test]
    fn liveness_matches_pid_and_start_time() {
        let recorded = [(7596, 185), (1900, 180), (9116, 1500), (8212, 523)];
        let start_of = |pid| recorded.iter().find(|(p, _)| *p == pid).map(|(_, s)| *s);
        let known = [
            (
                "classic",
                ProcessId {
                    pid: 7596,
                    start: 185,
                },
            ),
            (
                "terminal",
                ProcessId {
                    pid: 9116,
                    start: 1500,
                },
            ),
            (
                "reused",
                ProcessId {
                    pid: 1900,
                    start: 100,
                },
            ),
            (
                "exited",
                ProcessId {
                    pid: 30000,
                    start: 200,
                },
            ),
        ];
        assert_eq!(gone(known, start_of), ["reused", "exited"]);
        assert!(gone([], start_of).is_empty());
    }

    #[test]
    fn the_test_knock_toasts_even_when_suppressed_and_ends_after_ten_seconds() {
        let mut t = core("test-knock");
        t.hook(0, "real", "Stop", "", None);
        let last = read(&t.shared).last_event_at;
        for (n, suppress) in [None, Some("pause"), Some("quiet")].into_iter().enumerate() {
            match suppress {
                Some("pause") => {
                    t.core.handle(Msg::TogglePause, T0, NOON);
                }
                Some(_) => {
                    t.core.handle(Msg::TogglePause, T0, NOON);
                    let patch = json!({ "quietHours": { "enabled": true, "start": "00:00", "end": "23:59" } });
                    t.core
                        .handle(Msg::Settings { patch, reply: None }, T0, NOON);
                    assert!(
                        t.core.settings.suppressed(NOON) && !t.core.settings.pause_notifications
                    );
                }
                None => {}
            }
            let at = T0 + 100_000 * (n as u64 + 1);
            let id = format!("knock-test-{at}");
            let outs = t.core.handle(Msg::TestKnock, at, NOON);
            let want = (
                "Knock test needs you".to_string(),
                "Claude Code has a question for you.".to_string(),
            );
            assert_eq!(toasts(&outs), [want], "{suppress:?}");
            {
                let p = read(&t.shared);
                let v = p.sessions.sessions.iter().find(|v| v.id == id).unwrap();
                assert_eq!(
                    (v.tier, v.state, v.simulated),
                    ("needs_you", "needs_input", true)
                );
                assert_eq!(v.project, "Knock test");
                assert_eq!(p.last_event_at, last, "not a Claude Code event");
            }
            t.core.tick(at + 9_999, NOON);
            assert!(t.states().iter().any(|(s, _)| *s == id));
            t.core.tick(at + 10_000, NOON);
            assert!(!t.states().iter().any(|(s, _)| *s == id));
            assert_eq!(read(&t.shared).last_event_at, last);
        }
        // A real session's toast is still suppressed.
        assert!(
            toasts(&t.hook(
                400_000,
                "s2",
                "PermissionRequest",
                r#","tool_name":"Bash""#,
                None
            ))
            .is_empty()
        );
    }
}
