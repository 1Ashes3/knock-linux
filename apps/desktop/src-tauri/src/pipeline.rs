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
//! tray count. [`Published`] is what the server and the commands read.

use crate::jump::{Precision, ProcessId};
use crate::platform::{self, Platform};
use crate::settings::{self, Settings};
use knock_core::{
    Adapter, Category, ClaudeCode, Effect, Engine, Event, Input, Kind, NeedsKind, Reason, Tier,
    iso8601,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

const SNAPSHOT: &str = "sessions.json";
/// The test knock's project name and how long it stays.
const TEST_PROJECT: &str = "Knock test";
const TEST_MS: u64 = 10_000;

pub enum Msg {
    /// A hook request (§8.1), received at `ts` (ms since the Unix epoch).
    /// `owner` is the connection's process (§9.4 step 1), looked up before
    /// the reply; `None` if it wasn't found or `sim` is set.
    Hook {
        ts: u64,
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
    pub project: String,
    pub tier: &'static str,
    pub state: &'static str,
    pub since: String,
    pub activity: Option<Category>,
    pub needs: Option<NeedsView>,
    pub precision: Precision,
    pub animal: String,
    pub simulated: bool,
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
    adapter: ClaudeCode,
    settings: Settings,
    /// Last value sent as `SetSuppressed`.
    suppressed: bool,
    procs: HashMap<String, Found>,
    /// Sessions whose missing connection owner was already logged.
    unowned: HashSet<String>,
    /// A `Persist` not yet written; [`Core::flush`] writes it.
    dirty: Option<u64>,
    /// FR-8: sessions that became Ready, never shown. For Phase 4 (FR-9).
    #[cfg_attr(not(test), allow(dead_code))]
    ready: Vec<String>,
    /// Test-knock sessions and when they end.
    tests: Vec<(String, u64)>,
    view: SessionsChanged,
    needs_you: usize,
    last_event_at: Option<u64>,
    dir: PathBuf,
    platform: Option<Arc<dyn Platform>>,
    shared: Shared,
}

impl Core {
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
        let mut engine = Engine::new(settings.engine());
        let suppressed = settings.suppressed(local_minute);
        engine.step(Input::SetSuppressed(suppressed));
        let core = Core {
            engine,
            adapter: ClaudeCode { users: users() },
            settings,
            suppressed,
            procs: HashMap::new(),
            unowned: HashSet::new(),
            dirty: None,
            ready: Vec::new(),
            tests: Vec::new(),
            view: SessionsChanged::default(),
            needs_you: 0,
            last_event_at: None,
            dir,
            platform,
            shared,
        };
        core.publish();
        core
    }

    /// Handles one message. Suppression is re-checked first, so an event in
    /// the first second of quiet hours is already covered (§8.4).
    pub fn handle(&mut self, msg: Msg, now: u64, local_minute: u32) -> Vec<Out> {
        let mut run = Run::default();
        self.suppress(local_minute, &mut run);
        match msg {
            Msg::Hook {
                ts,
                body,
                sim,
                owner,
            } => self.hook(ts, &body, sim, owner, &mut run),
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
        self.finish(run, now)
    }

    fn hook(&mut self, ts: u64, body: &[u8], sim: bool, owner: Option<ProcessId>, run: &mut Run) {
        self.last_event_at = Some(ts);
        let mut event = match self.adapter.parse(body, ts) {
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
        self.hook(now, body.to_string().as_bytes(), true, None, run);
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
        if let Some(on) = sign_in {
            self.sign_in(on).map_err(|_| {
                "Knock couldn't change whether it starts when you sign in. Please try again."
                    .to_string()
            })?;
        }
        if let Err(e) = settings::save(&self.dir, &new) {
            tracing::warn!(kind = ?e.kind(), "settings not saved");
            let was = self.settings.launch_at_sign_in.unwrap_or(false);
            if sign_in.is_some_and(|on| on != was) && self.sign_in(was).is_err() {
                tracing::warn!("launch at sign-in not undone");
            }
            return Err("Knock couldn't save the settings.".into());
        }
        self.apply_settings(new.clone(), local_minute, run);
        Ok(new)
    }

    /// Adds or removes the sign-in entry. The error can name the exe's path,
    /// so only its kind is logged (§11.5).
    fn sign_in(&self, on: bool) -> std::io::Result<()> {
        let p = (self.platform.as_ref()).ok_or_else(|| std::io::Error::other("no platform"))?;
        let r = p.set_launch_at_sign_in(on);
        match &r {
            Ok(()) => tracing::info!(on, "launch at sign-in set"),
            Err(e) => tracing::warn!(on, kind = ?e.kind(), "launch at sign-in not changed"),
        }
        r
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
        self.engine.set_settings(new.engine());
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
                project: s.project.clone(),
                tier: s.state.tier().as_str(),
                state: s.state.as_str(),
                since: iso8601(s.since),
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

/// `%USERNAME%` and the last part of `%USERPROFILE%`, for the adapter's
/// `home` rule (ADR 0020). Process environment, not a Windows API call.
fn users() -> Vec<String> {
    ["USERNAME", "USERPROFILE"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .filter_map(|v| {
            v.rsplit(['\\', '/'])
                .find(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect()
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
                body: body(session, hook, extra),
                sim: false,
                owner,
            };
            self.core.handle(msg, T0 + t, self.minute)
        }
        fn sim(&mut self, t: u64, session: &str, hook: &str, extra: &str) -> Vec<Out> {
            let msg = Msg::Hook {
                ts: T0 + t,
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
            project: "repo-name".into(),
            tier: "needs_you",
            state: "needs_input",
            since: iso8601(T0),
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
                r#"{"rev":3,"sessions":[{"id":"s1","project":"repo-name","tier":"needs_you","#,
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
        let logs = temp_dir("owner-log");
        let mut t = core("owner-log");
        let sub = crate::log::FileLog::new(logs.clone());
        tracing::subscriber::with_default(sub, || {
            for i in 0..5 {
                t.hook(i, "a", "PreToolUse", r#","tool_name":"Read""#, None);
            }
            t.hook(10, "b", "Stop", "", None);
            t.hook(20, "a", "UserPromptSubmit", "", Some(P1));
            t.hook(30, "a", "Stop", "", None);
        });
        let text: String = std::fs::read_dir(&logs)
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        let lines: Vec<_> = text
            .lines()
            .filter(|l| l.contains("connection owner not found"))
            .collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains(r#"session="a""#) && lines[1].contains(r#"session="b""#));
        let _ = std::fs::remove_dir_all(&logs);
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
        // The save fails after the entry changed: the change is undone.
        let block = t.dir.join("settings.json.tmp");
        std::fs::create_dir(&block).unwrap();
        let e = set(&mut t, json!({ "launchAtSignIn": false })).unwrap_err();
        assert_eq!(e, "Knock couldn't save the settings.");
        assert_eq!(calls(&t), [true, true, false, false, true], "undone");
        assert_eq!(saved(&t), Some(true));
        assert_eq!(read(&t.shared).settings.launch_at_sign_in, Some(true));
        // Same value again: nothing to undo.
        assert!(set(&mut t, json!({ "launchAtSignIn": true })).is_err());
        assert_eq!(calls(&t).len(), 6);
        std::fs::remove_dir(&block).unwrap();
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
        assert_eq!(s["needs"]["label"], "Bash");
        assert_eq!(s["process"]["pid"], P1.pid);
        assert_eq!(s["process"]["owner"], "WindowsTerminal.exe");
        let mut keys: Vec<_> = s.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "activity",
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
