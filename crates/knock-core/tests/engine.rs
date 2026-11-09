//! §12.1 Engine: every §8.4 transition, timer, dedup, suppression and seen
//! rule (ADR 0010, 0016, 0018, 0019, 0021).

use knock_core::{
    ANIMALS, Category, Context, Detail, Effect, Engine, Event, Input, Kind, NeedsKind, Reason,
    Settings, State, project_key,
};

const MIN: u64 = 60_000;

fn ev(kind: Kind, sid: &str, ts: u64) -> Event {
    Event {
        v: 1,
        agent: "claude-code",
        session_id: sid.into(),
        ts,
        kind,
        detail: Detail::default(),
        context: Context {
            cwd: r"C:\code\demo".into(),
            project: "demo".into(),
            pid: None,
            agent_id: None,
        },
    }
}

fn tool(mut e: Event, t: &str) -> Event {
    e.detail.tool = Some(t.into());
    e.detail.category = Some(Category::of(t));
    e
}

fn agent(mut e: Event, a: &str) -> Event {
    e.context.agent_id = Some(a.into());
    e
}

/// The app found the session's process (§9.4).
fn pid(mut e: Event) -> Event {
    e.context.pid = Some(4242);
    e
}

/// A PermissionRequest (has a tool) and the `permission_prompt` Notification (none).
fn request(sid: &str, ts: u64) -> Event {
    tool(ev(Kind::NeedsPermission, sid, ts), "Write")
}
fn prompt(sid: &str, ts: u64) -> Event {
    ev(Kind::NeedsPermission, sid, ts)
}

struct H(Engine);

impl H {
    fn new() -> Self {
        H(Engine::new(Settings::default()))
    }
    fn with(s: Settings) -> Self {
        H(Engine::new(s))
    }
    fn send(&mut self, e: Event) -> Vec<Effect> {
        self.0.step(Input::Event {
            event: e,
            simulated: false,
        })
    }
    fn kind(&mut self, k: Kind, ts: u64) -> Vec<Effect> {
        self.send(ev(k, "s1", ts))
    }
    fn tick(&mut self, now: u64) -> Vec<Effect> {
        self.0.step(Input::Tick { now })
    }
    /// Pause or quiet hours on or off; changes no state, so no effects.
    fn suppress(&mut self, on: bool) {
        assert_eq!(self.0.step(Input::SetSuppressed(on)), vec![]);
    }
    fn waiting(&self) -> Vec<Option<&str>> {
        let n = self.s1().needs.as_ref().unwrap();
        n.waiting().iter().map(Option::as_deref).collect()
    }
    fn gone(&mut self, now: u64) -> Vec<Effect> {
        self.0.step(Input::ProcessGone {
            session_id: "s1".into(),
            now,
        })
    }
    fn state(&self) -> Option<State> {
        self.0
            .sessions()
            .iter()
            .find(|s| s.id == "s1")
            .map(|s| s.state)
    }
    fn s1(&self) -> &knock_core::Session {
        self.0.sessions().iter().find(|s| s.id == "s1").unwrap()
    }
    /// Puts s1 into `state` at time `t` (via the shortest real path).
    fn into(state: State, t: u64) -> Self {
        let mut h = H::new();
        match state {
            State::Idle => h.kind(Kind::SessionStarted, t),
            State::Working => h.kind(Kind::TurnStarted, t),
            State::Ready => h.kind(Kind::TurnCompleted, t),
            State::NeedsInput => h.send(request("s1", t)),
            State::Failed => h.kind(Kind::AgentFailed, t),
            State::Stalled => {
                h.kind(Kind::TurnStarted, t - 10 * MIN);
                h.tick(t)
            }
            State::EndedUnexpectedly => {
                h.kind(Kind::TurnStarted, t);
                h.gone(t)
            }
            _ => unreachable!(),
        };
        assert_eq!(h.state(), Some(state));
        h
    }
}

fn notify(reason: Reason) -> Effect {
    Effect::Notify {
        session_id: "s1".into(),
        reason,
    }
}
fn ready_add() -> Effect {
    Effect::ReadyBatchAdd {
        session_id: "s1".into(),
    }
}
fn escalate() -> Effect {
    Effect::Escalate {
        session_id: "s1".into(),
    }
}
use Effect::Persist;

const V1: [State; 7] = [
    State::Idle,
    State::Working,
    State::Ready,
    State::NeedsInput,
    State::Failed,
    State::Stalled,
    State::EndedUnexpectedly,
];

// (none) + session.started or any first event → idle, or the state it implies.
#[test]
fn first_event_creates_the_session_in_the_state_it_implies() {
    use Reason::*;
    for (kind, state, fx) in [
        (Kind::SessionStarted, Some(State::Idle), vec![Persist]),
        (Kind::TurnIdle, Some(State::Idle), vec![Persist]),
        (Kind::TurnStarted, Some(State::Working), vec![Persist]),
        (Kind::Activity, Some(State::Working), vec![Persist]),
        (
            Kind::NeedsPermission,
            Some(State::NeedsInput),
            vec![notify(NeedsPermission), Persist],
        ),
        (
            Kind::NeedsAnswer,
            Some(State::NeedsInput),
            vec![notify(NeedsAnswer), Persist],
        ),
        (
            Kind::TurnCompleted,
            Some(State::Ready),
            vec![ready_add(), Persist],
        ),
        (
            Kind::AgentFailed,
            Some(State::Failed),
            vec![notify(Failed), Persist],
        ),
        (Kind::SessionEnded, None, vec![]),
        (Kind::UserAck, None, vec![]),
        (Kind::UserDismiss, None, vec![]),
    ] {
        let mut h = H::new();
        assert_eq!(h.kind(kind, 5), fx, "{kind:?}");
        assert_eq!(h.state(), state, "{kind:?}");
    }
}

#[test]
fn a_new_session_keeps_what_session_view_needs_and_not_the_cwd() {
    let mut h = H::new();
    let mut e = tool(ev(Kind::Activity, "s1", 7), "PowerShell");
    e.context.cwd = r"D:\Work\Secret-Path\demo".into();
    h.0.step(Input::Event {
        event: e,
        simulated: true,
    });
    let s = h.s1();
    assert_eq!(s.project, "demo");
    assert_eq!(s.project_key, project_key(r"d:\work\secret-path\demo"));
    assert_eq!((s.state, s.since, s.last_event), (State::Working, 7, 7));
    assert_eq!(s.activity, Some(Category::Run));
    assert_eq!(s.animal, "fox");
    assert!(s.simulated);
    assert!(
        !format!("{s:?}").contains("Secret-Path"),
        "cwd kept (§11.4)"
    );
}

// idle, ready + turn.started → working.
#[test]
fn turn_started_from_idle_or_ready_works() {
    for from in [State::Idle, State::Ready] {
        let mut h = H::into(from, 1000);
        assert_eq!(h.kind(Kind::TurnStarted, 2000), vec![Persist]);
        assert_eq!(h.state(), Some(State::Working));
        assert_eq!(h.s1().since, 2000);
    }
}

// working, stalled + activity → working; reset stall timer.
#[test]
fn activity_keeps_working_and_resets_the_stall_timer() {
    let mut h = H::into(State::Working, 0);
    assert_eq!(
        h.send(tool(ev(Kind::Activity, "s1", 9 * MIN), "Read")),
        vec![]
    );
    assert_eq!(h.s1().activity, Some(Category::Read));
    assert_eq!(h.tick(10 * MIN), vec![], "reset by the activity");
    assert_eq!(h.tick(19 * MIN), vec![Persist]);
    assert_eq!(h.state(), Some(State::Stalled));
    assert_eq!(h.kind(Kind::Activity, 19 * MIN + 1), vec![Persist]);
    assert_eq!(h.state(), Some(State::Working));
    assert_eq!(h.tick(29 * MIN), vec![], "reset again");
}

// working, idle, ready + needs.* → needs_input; new episode; Notify.
#[test]
fn needs_from_working_idle_or_ready_opens_an_episode_and_notifies() {
    for from in [State::Working, State::Idle, State::Ready] {
        let mut h = H::into(from, 0);
        assert_eq!(
            h.send(request("s1", 100)),
            vec![notify(Reason::NeedsPermission), Persist]
        );
        let n = h.s1().needs.clone().unwrap();
        assert_eq!(
            (n.kind, n.label.as_deref(), n.seen),
            (NeedsKind::Permission, Some("Write"), false)
        );
        let mut h = H::into(from, 0);
        assert_eq!(
            h.kind(Kind::NeedsAnswer, 100),
            vec![notify(Reason::NeedsAnswer), Persist]
        );
        let n = h.s1().needs.clone().unwrap();
        assert_eq!((n.kind, n.label), (NeedsKind::Answer, None));
    }
}

// needs_input + needs.* → same episode, no effects; kind set by the opener.
#[test]
fn more_needs_in_an_open_episode_change_nothing() {
    let mut h = H::new();
    h.kind(Kind::NeedsAnswer, 0);
    assert_eq!(h.send(request("s1", 60_000)), vec![]);
    assert_eq!(h.send(prompt("s1", 61_000)), vec![]);
    assert_eq!(h.kind(Kind::NeedsAnswer, 62_000), vec![]);
    let n = h.s1().needs.clone().unwrap();
    assert_eq!((n.kind, n.opened), (NeedsKind::Answer, 0));
}

// idle, ready + activity → unchanged, no effects (ADR 0010).
#[test]
fn background_activity_in_idle_or_ready_changes_nothing() {
    for from in [State::Idle, State::Ready] {
        let mut h = H::into(from, 0);
        assert_eq!(h.send(agent(ev(Kind::Activity, "s1", 1), "a1")), vec![]);
        assert_eq!(h.kind(Kind::Activity, 2), vec![]);
        assert_eq!(h.state(), Some(from));
        assert_eq!(h.s1().since, 0);
    }
}

// needs_input + activity from the same agent, or turn.started → working; close.
#[test]
fn same_agent_activity_or_a_prompt_closes_the_episode() {
    // Opened by the main agent: only main-agent activity closes it.
    let mut h = H::into(State::NeedsInput, 0);
    assert_eq!(h.send(agent(ev(Kind::Activity, "s1", 1), "a1")), vec![]);
    assert_eq!(h.state(), Some(State::NeedsInput));
    assert_eq!(h.kind(Kind::Activity, 2), vec![Persist]);
    assert_eq!(
        (h.state(), h.s1().needs.clone()),
        (Some(State::Working), None)
    );
    // Opened by a1: neither the main agent nor a2 closes it.
    let mut h = H::into(State::Ready, 0);
    h.send(agent(request("s1", 1), "a1"));
    assert_eq!(h.kind(Kind::Activity, 2), vec![]);
    assert_eq!(h.send(agent(ev(Kind::Activity, "s1", 3), "a2")), vec![]);
    assert_eq!(h.state(), Some(State::NeedsInput));
    assert_eq!(
        h.send(agent(ev(Kind::Activity, "s1", 4), "a1")),
        vec![ready_add(), Persist]
    );
    // Opened from ready, the main turn was done: back to ready (ADR 0021).
    assert_eq!(h.state(), Some(State::Ready));
    // turn.started closes it whoever opened it.
    let mut h = H::into(State::Ready, 0);
    h.send(agent(request("s1", 1), "a1"));
    assert_eq!(h.kind(Kind::TurnStarted, 2), vec![Persist]);
    assert_eq!(h.state(), Some(State::Working));
}

// needs_input + turn.completed → ready; close; ReadyBatchAdd.
// working, stalled + turn.completed → ready; ReadyBatchAdd.
#[test]
fn turn_completed_is_ready_and_batched() {
    for from in [State::NeedsInput, State::Working, State::Stalled] {
        let mut h = H::into(from, 10 * MIN);
        assert_eq!(
            h.kind(Kind::TurnCompleted, 11 * MIN),
            vec![ready_add(), Persist]
        );
        assert_eq!(
            (h.state(), h.s1().needs.clone()),
            (Some(State::Ready), None)
        );
    }
}

// ready + turn.idle → ready; none.
#[test]
fn idle_prompt_keeps_ready() {
    let mut h = H::into(State::Ready, 0);
    assert_eq!(h.kind(Kind::TurnIdle, 60_000), vec![]);
    assert_eq!(h.state(), Some(State::Ready));
}

// working + Tick: no events for stallMinutes → stalled; Notify only if notifyOnStall.
#[test]
fn working_stalls_after_stall_minutes() {
    let mut h = H::into(State::Working, 0);
    assert_eq!(h.tick(10 * MIN - 1), vec![]);
    assert_eq!(h.tick(10 * MIN), vec![Persist]);
    assert_eq!((h.state(), h.s1().since), (Some(State::Stalled), 10 * MIN));
    assert_eq!(h.tick(30 * MIN), vec![], "once");

    let mut h = H::with(Settings {
        stall_ms: 2 * MIN,
        notify_on_stall: true,
        ..Settings::default()
    });
    h.kind(Kind::TurnStarted, 0);
    assert_eq!(h.tick(2 * MIN), vec![notify(Reason::Stalled), Persist]);
    assert_eq!(h.tick(3 * MIN), vec![]);
    // User actions are not progress: they don't reset the stall timer.
    h.kind(Kind::Activity, 4 * MIN);
    h.kind(Kind::UserAck, 5 * MIN);
    assert_eq!(h.tick(6 * MIN), vec![notify(Reason::Stalled), Persist]);
}

// needs_input + Tick: open for escalationMinutes, not escalated, not seen → Escalate once.
#[test]
fn escalates_once_per_episode() {
    let mut h = H::into(State::NeedsInput, 0);
    assert_eq!(h.tick(5 * MIN - 1), vec![]);
    assert_eq!(h.tick(5 * MIN), vec![escalate(), Persist]);
    assert_eq!(h.tick(10 * MIN), vec![]);
    assert_eq!(h.tick(60 * MIN), vec![]);
    // A new episode escalates again.
    h.kind(Kind::Activity, 61 * MIN);
    h.send(request("s1", 62 * MIN));
    assert_eq!(h.tick(67 * MIN), vec![escalate(), Persist]);
    // Needs-input sessions never stall.
    assert_eq!(h.tick(200 * MIN), vec![]);
    assert_eq!(h.state(), Some(State::NeedsInput));
}

// needs_input + user.ack → needs_input, seen; no Escalate for this episode (ADR 0016).
#[test]
fn ack_marks_the_episode_seen_and_stops_escalation() {
    let mut h = H::into(State::NeedsInput, 0);
    assert_eq!(h.kind(Kind::UserAck, 1000), vec![Persist]);
    let s = h.s1();
    assert_eq!((s.state, s.since), (State::NeedsInput, 0));
    assert!(s.needs.as_ref().unwrap().seen);
    assert_eq!(h.kind(Kind::UserAck, 2000), vec![], "already seen");
    assert_eq!(h.tick(5 * MIN), vec![]);
    assert_eq!(h.tick(60 * MIN), vec![]);
    // It closes as before; the next episode is unseen and knocks again.
    assert_eq!(h.kind(Kind::TurnStarted, 61 * MIN), vec![Persist]);
    assert_eq!(
        h.send(request("s1", 62 * MIN)),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    assert!(!h.s1().needs.as_ref().unwrap().seen);
    assert_eq!(h.tick(67 * MIN), vec![escalate(), Persist]);
}

// any + agent.failed → failed; Notify.
#[test]
fn failure_from_any_state_notifies_once() {
    for from in V1 {
        let mut h = H::into(from, 10 * MIN);
        let mut e = ev(Kind::AgentFailed, "s1", 11 * MIN);
        e.detail.reason = Some("rate_limit".into());
        let want = if from == State::Failed {
            vec![]
        } else {
            vec![notify(Reason::Failed), Persist]
        };
        assert_eq!(h.send(e), want, "{from:?}");
        assert_eq!(
            (h.state(), h.s1().needs.clone()),
            (Some(State::Failed), None)
        );
    }
}

// any except failed + process gone → ended_unexpectedly; none; fades after 60 s.
#[test]
fn process_gone_ends_unexpectedly_then_fades() {
    for from in V1 {
        let mut h = H::into(from, 10 * MIN);
        let fx = h.gone(11 * MIN);
        if from == State::Failed || from == State::EndedUnexpectedly {
            assert_eq!(fx, vec![]);
            assert_eq!(h.state(), Some(from));
            continue;
        }
        assert_eq!(fx, vec![Persist], "{from:?}");
        assert_eq!(
            (h.state(), h.s1().needs.clone()),
            (Some(State::EndedUnexpectedly), None)
        );
        assert_eq!(h.tick(12 * MIN - 1), vec![]);
        assert_eq!(h.tick(12 * MIN), vec![Persist]);
        assert_eq!(h.state(), None, "faded and removed");
    }
}

// ready + user.ack → idle.
#[test]
fn ack_on_ready_rests() {
    let mut h = H::into(State::Ready, 0);
    assert_eq!(h.kind(Kind::UserAck, 1), vec![Persist]);
    assert_eq!(h.state(), Some(State::Idle));
}

// failed, ended_unexpectedly + user.dismiss → removed; nothing else is dismissed.
#[test]
fn dismiss_removes_failed_or_ended_only() {
    for from in V1 {
        let mut h = H::into(from, 10 * MIN);
        let fx = h.kind(Kind::UserDismiss, 10 * MIN + 1);
        if matches!(from, State::Failed | State::EndedUnexpectedly) {
            assert_eq!((fx, h.state()), (vec![Persist], None));
        } else {
            assert_eq!((fx, h.state()), (vec![], Some(from)));
        }
    }
}

// any + session.ended → removed. New events with the same id start it again
// (`claude --continue`).
#[test]
fn session_end_removes_and_continue_starts_again() {
    for from in V1 {
        let mut h = H::into(from, 10 * MIN);
        assert_eq!(h.kind(Kind::SessionEnded, 11 * MIN), vec![Persist]);
        assert_eq!(h.state(), None);
        assert_eq!(h.kind(Kind::TurnStarted, 12 * MIN), vec![Persist]);
        assert_eq!((h.state(), h.s1().since), (Some(State::Working), 12 * MIN));
    }
}

// A session with no events and no live process for 6 hours is removed.
#[test]
fn removed_after_six_hours_without_events_or_process() {
    let six = 6 * 60 * MIN;
    let mut h = H::new();
    h.send(pid(ev(Kind::AgentFailed, "s1", 0)));
    h.gone(MIN);
    assert_eq!(h.tick(MIN + six - 1), vec![]);
    assert_eq!(h.tick(MIN + six), vec![Persist]);
    assert_eq!(h.state(), None);
    // A live process (known, never reported gone) keeps it.
    let mut h = H::new();
    h.send(pid(ev(Kind::AgentFailed, "s1", 0)));
    assert_eq!(h.tick(10 * six), vec![]);
    assert_eq!(h.state(), Some(State::Failed));
    // An event after the process went counts: 6 h from the last of the two.
    let mut h = H::new();
    h.send(pid(ev(Kind::AgentFailed, "s1", 0)));
    h.gone(MIN);
    h.kind(Kind::TurnIdle, 2 * MIN);
    assert_eq!(h.tick(MIN + six), vec![]);
    assert_eq!(h.tick(2 * MIN + six), vec![Persist]);
}

// ADR 0018 item 7: a session whose process was never found counts as having
// no live process: removed 6 h after its last event, in any state.
#[test]
fn a_session_with_no_known_process_is_removed_six_hours_after_its_last_event() {
    let six = 6 * 60 * MIN;
    for from in [State::Idle, State::NeedsInput, State::Ready, State::Failed] {
        let mut h = H::into(from, 0);
        h.kind(Kind::UserAck, 0); // seen: no escalation; ready: idle
        h.send(ev(Kind::TurnIdle, "s1", MIN));
        assert_eq!(h.tick(MIN + six - 1), vec![], "{from:?}");
        assert_eq!(h.tick(MIN + six), vec![Persist], "{from:?}");
        assert_eq!(h.state(), None, "{from:?}");
    }
    // A pid on a later event makes the process known.
    let mut h = H::into(State::Ready, 0);
    h.send(pid(ev(Kind::TurnIdle, "s1", MIN)));
    assert_eq!(h.tick(10 * six), vec![]);
    assert_eq!(h.state(), Some(State::Ready));
    // User actions are not events: they don't restart the 6 h.
    let mut h = H::into(State::Ready, 0);
    h.kind(Kind::UserAck, 5 * 60 * MIN);
    assert_eq!(h.tick(six), vec![Persist]);
    assert_eq!(h.state(), None);
}

// Dedup: PermissionRequest + permission_prompt within 10 s = one episode,
// one Notify, even if the episode closed in between (ADR 0010).
#[test]
fn prompt_within_ten_seconds_of_a_request_is_the_same_episode() {
    // Open: same episode.
    let mut h = H::into(State::Working, 0);
    assert_eq!(h.send(request("s1", 1000)).len(), 2);
    assert_eq!(h.send(prompt("s1", 7000)), vec![]);
    // Closed in between (fast approval, then Stop): still nothing.
    let mut h = H::into(State::Working, 0);
    h.send(request("s1", 1000));
    h.kind(Kind::Activity, 2000);
    h.kind(Kind::TurnCompleted, 3000);
    assert_eq!(h.send(prompt("s1", 11_000)), vec![], "10 s, inclusive");
    assert_eq!(h.state(), Some(State::Ready));
    // Just past 10 s: a new episode, which the prompt opens.
    assert_eq!(
        h.send(prompt("s1", 11_001)),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    assert_eq!(h.s1().needs.as_ref().unwrap().label, None);
    // A question's request pairs with the prompt too.
    let mut h = H::into(State::Working, 0);
    h.kind(Kind::NeedsAnswer, 1000);
    h.kind(Kind::Activity, 2000);
    assert_eq!(h.send(prompt("s1", 7000)), vec![]);
    assert_eq!(h.state(), Some(State::Working));
    // Other sessions don't pair.
    let mut h = H::into(State::Working, 0);
    h.send(request("s2", 1000));
    assert_eq!(h.send(prompt("s1", 2000)).len(), 2);
}

#[test]
fn a_second_request_right_after_a_prompt_still_knocks() {
    let mut h = H::into(State::Working, 0);
    h.send(request("s1", 0));
    h.send(prompt("s1", 6000));
    h.kind(Kind::Activity, 7000);
    assert_eq!(
        h.send(request("s1", 8000)),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    // Two requests in a row are two episodes.
    h.kind(Kind::Activity, 9000);
    assert_eq!(h.send(request("s1", 9500)).len(), 2);
}

// Suppression: Notify and Escalate are suppressed while paused or in quiet
// hours; the state (and so the animation) still changes.
#[test]
fn suppression_drops_notify_and_escalate_but_not_states() {
    let mut h = H::with(Settings {
        notify_on_stall: true,
        ..Settings::default()
    });
    h.kind(Kind::TurnStarted, 0);
    h.suppress(true);
    assert_eq!(h.send(request("s1", 2000)), vec![Persist]);
    assert_eq!(h.state(), Some(State::NeedsInput));
    assert_eq!(h.tick(2000 + 5 * MIN), vec![Persist], "escalation dropped");
    assert!(h.s1().needs.as_ref().unwrap().escalated);
    h.suppress(false);
    assert_eq!(h.tick(2000 + 6 * MIN), vec![], "dropped, not deferred");
    // Failed and stalled too.
    h.suppress(true);
    assert_eq!(h.kind(Kind::AgentFailed, 7 * MIN), vec![Persist]);
    let mut h = H::with(Settings {
        notify_on_stall: true,
        ..Settings::default()
    });
    h.kind(Kind::TurnStarted, 0);
    h.suppress(true);
    assert_eq!(h.tick(10 * MIN), vec![Persist]);
    assert_eq!(h.state(), Some(State::Stalled));
    // Lifting it doesn't replay what was dropped, but new episodes notify.
    h.suppress(false);
    assert_eq!(h.tick(11 * MIN), vec![]);
    h.kind(Kind::Activity, 12 * MIN);
    assert_eq!(h.send(request("s1", 13 * MIN)).len(), 2);
    // ReadyBatchAdd isn't a notification and is never suppressed.
    h.suppress(true);
    assert_eq!(
        h.kind(Kind::TurnCompleted, 14 * MIN),
        vec![ready_add(), Persist]
    );
}

#[test]
fn animals_come_from_the_list_in_order_and_overrides_win() {
    let mut s = Settings {
        animals: vec!["fox".into(), "owl".into()],
        ..Settings::default()
    };
    s.animal_overrides
        .insert(project_key(r"C:\CODE\Pinned"), "heron".into());
    let mut h = H::with(s);
    for (sid, cwd) in [
        ("s1", r"C:\code\a"),
        ("s2", r"C:\code\b"),
        ("s3", r"C:\code\c"),
        ("s4", r"c:\code\pinned"),
    ] {
        let mut e = ev(Kind::TurnStarted, sid, 0);
        e.context.cwd = cwd.into();
        h.send(e);
    }
    h.kind(Kind::SessionEnded, 1);
    let mut e = ev(Kind::TurnStarted, "s5", 2);
    e.context.cwd = r"C:\code\e".into();
    h.send(e);
    let animals: Vec<_> =
        h.0.sessions()
            .iter()
            .map(|s| (s.id.as_str(), s.animal.as_str()))
            .collect();
    assert_eq!(
        animals,
        [("s2", "owl"), ("s3", "fox"), ("s4", "heron"), ("s5", "owl")]
    );
    assert_eq!(H::into(State::Idle, 0).s1().animal, "fox", "default list");
}

/// Starts session `sid` in its own project folder.
fn start(h: &mut H, sid: &str, ts: u64) {
    let mut e = ev(Kind::SessionStarted, sid, ts);
    e.context.cwd = format!(r"C:\code\{sid}");
    h.send(e);
}

fn animals(h: &H) -> Vec<(&str, &str)> {
    let s = h.0.sessions().iter();
    s.map(|s| (s.id.as_str(), s.animal.as_str())).collect()
}

// §8.7, ADR 0030: the default list is the v1 animals in order.
#[test]
fn the_v1_animals_are_assigned_fox_rabbit_cat_owl() {
    assert_eq!(ANIMALS, ["fox", "rabbit", "cat", "owl"]);
    assert_eq!(Settings::default().animals, ANIMALS);
    let mut h = H::new();
    for (i, sid) in ["s1", "s2", "s3", "s4"].into_iter().enumerate() {
        start(&mut h, sid, i as u64);
    }
    assert_eq!(
        animals(&h),
        [
            ("s1", "fox"),
            ("s2", "rabbit"),
            ("s3", "cat"),
            ("s4", "owl")
        ]
    );
}

// ADR 0038: switching sprite packs re-casts running sessions at once.
#[test]
fn a_new_animal_list_recasts_running_sessions_in_order() {
    let mut h = H::new();
    for (i, sid) in ["s1", "s2", "s3"].into_iter().enumerate() {
        start(&mut h, sid, i as u64);
    }
    let pack = |list: &[&str]| Settings {
        animals: list.iter().map(|a| a.to_string()).collect(),
        ..Settings::default()
    };
    h.0.set_settings(pack(&["rose", "tulip"]));
    assert_eq!(
        animals(&h),
        [("s1", "rose"), ("s2", "tulip"), ("s3", "rose")]
    );
    // Other settings changes leave the cast alone.
    let mut quieter = pack(&["rose", "tulip"]);
    quieter.stall_ms += MIN;
    h.0.set_settings(quieter);
    assert_eq!(
        animals(&h),
        [("s1", "rose"), ("s2", "tulip"), ("s3", "rose")]
    );
    h.0.set_settings(Settings::default());
    assert_eq!(
        animals(&h),
        [("s1", "fox"), ("s2", "rabbit"), ("s3", "cat")]
    );
}

#[test]
fn a_fifth_session_wraps_around_to_the_fox() {
    let mut h = H::new();
    for (i, sid) in ["s1", "s2", "s3", "s4", "s5"].into_iter().enumerate() {
        start(&mut h, sid, i as u64);
    }
    assert_eq!(animals(&h)[4], ("s5", "fox"));
}

#[test]
fn a_freed_animal_is_reused_first() {
    let mut h = H::new();
    for (i, sid) in ["s1", "s2", "s3", "s4"].into_iter().enumerate() {
        start(&mut h, sid, i as u64);
    }
    h.send(ev(Kind::SessionEnded, "s2", 10));
    start(&mut h, "s5", 11);
    assert_eq!(
        animals(&h),
        [
            ("s1", "fox"),
            ("s3", "cat"),
            ("s4", "owl"),
            ("s5", "rabbit")
        ]
    );
    // A dismissed failed session frees its animal too.
    h.send(ev(Kind::AgentFailed, "s3", 12));
    h.send(ev(Kind::UserDismiss, "s3", 13));
    start(&mut h, "s6", 14);
    assert_eq!(animals(&h)[3], ("s6", "cat"));
}

#[test]
fn an_override_wins_over_the_order() {
    let mut s = Settings::default();
    s.animal_overrides
        .insert(project_key(r"C:\code\s1"), "owl".into());
    let mut h = H::with(s);
    for (i, sid) in ["s1", "s2", "s3", "s4"].into_iter().enumerate() {
        start(&mut h, sid, i as u64);
    }
    assert_eq!(
        animals(&h),
        [
            ("s1", "owl"),
            ("s2", "fox"),
            ("s3", "rabbit"),
            ("s4", "cat")
        ]
    );
}

#[test]
fn persist_only_when_the_snapshot_changes() {
    let mut h = H::into(State::Working, 0);
    assert_eq!(h.send(tool(ev(Kind::Activity, "s1", 1), "Bash")), vec![]);
    assert_eq!(h.kind(Kind::TurnIdle, 2), vec![]);
    assert_eq!(h.tick(1000), vec![]);
    assert_eq!(
        h.0.step(Input::ProcessGone {
            session_id: "nope".into(),
            now: 3
        }),
        vec![]
    );
    assert_eq!(h.send(ev(Kind::UserAck, "nope", 4)), vec![]);
}

// ADR 0018 item 1: a new request while a seen episode is open opens a new,
// unseen episode with its own Notify. Unseen, it stays the same episode.
#[test]
fn a_new_request_after_the_episode_was_seen_knocks_again() {
    let question = |sid: &str, ts| ev(Kind::NeedsAnswer, sid, ts);
    for (new, reason) in [
        (request as fn(&str, u64) -> Event, Reason::NeedsPermission),
        (question, Reason::NeedsAnswer),
    ] {
        let mut h = H::into(State::NeedsInput, 0);
        // Unseen: same episode, no effects.
        assert_eq!(h.send(new("s1", MIN)), vec![], "{reason:?}");
        h.kind(Kind::UserAck, 2 * MIN);
        assert_eq!(h.send(new("s1", 3 * MIN)), vec![notify(reason), Persist]);
        let n = h.s1().needs.clone().unwrap();
        assert_eq!((n.seen, n.opened, n.escalated), (false, 3 * MIN, false));
        assert_eq!(h.s1().since, 0, "still needs_input, so `since` stays");
        // The new episode is unseen again and escalates on its own clock.
        assert_eq!(h.send(new("s1", 4 * MIN)), vec![], "{reason:?}");
        assert_eq!(h.tick(8 * MIN - 1), vec![]);
        assert_eq!(h.tick(8 * MIN), vec![escalate(), Persist]);
    }
    // The permission_prompt Notification is never a new request,
    // deduplicated (within 10 s) or not.
    let mut h = H::into(State::NeedsInput, 0);
    h.kind(Kind::UserAck, 1000);
    assert_eq!(h.send(prompt("s1", 6000)), vec![]);
    assert_eq!(h.send(prompt("s1", 2 * MIN)), vec![]);
    assert!(h.s1().needs.as_ref().unwrap().seen);
    // Any request after the click counts, even in the same ms.
    let mut h = H::into(State::NeedsInput, 0);
    h.kind(Kind::UserAck, 0);
    assert_eq!(h.send(request("s1", 0)).len(), 2);
}

// ADR 0018 item 2: stalled + needs.* → needs_input, new episode, Notify.
#[test]
fn needs_while_stalled_opens_an_episode() {
    for (e, reason) in [
        (request("s1", 11 * MIN), Reason::NeedsPermission),
        (ev(Kind::NeedsAnswer, "s1", 11 * MIN), Reason::NeedsAnswer),
        (prompt("s1", 11 * MIN), Reason::NeedsPermission),
    ] {
        let mut h = H::into(State::Stalled, 10 * MIN);
        assert_eq!(h.send(e), vec![notify(reason), Persist]);
        let s = h.s1();
        assert_eq!((s.state, s.since), (State::NeedsInput, 11 * MIN));
        assert!(!s.needs.as_ref().unwrap().seen);
    }
}

// ADR 0018 item 3: stalled + turn.started → working.
#[test]
fn a_prompt_while_stalled_works() {
    let mut h = H::into(State::Stalled, 10 * MIN);
    assert_eq!(h.kind(Kind::TurnStarted, 11 * MIN), vec![Persist]);
    assert_eq!((h.state(), h.s1().since), (Some(State::Working), 11 * MIN));
    assert_eq!(h.tick(21 * MIN - 1), vec![], "stall timer restarted");
    assert_eq!(h.tick(21 * MIN), vec![Persist]);
    assert_eq!(h.state(), Some(State::Stalled));
}

// ADR 0018 item 4: failed + turn.started or main-agent activity (ADR 0019)
// → working; failed + needs.* → needs_input with Notify. Dismiss still
// removes it (pinned in `dismiss_removes_failed_or_ended_only`).
#[test]
fn a_retry_after_failure_works_and_can_knock() {
    let mut h = H::into(State::Failed, 0);
    assert_eq!(h.kind(Kind::TurnStarted, MIN), vec![Persist]);
    assert_eq!((h.state(), h.s1().since), (Some(State::Working), MIN));
    let mut h = H::into(State::Failed, 0);
    assert_eq!(
        h.send(tool(ev(Kind::Activity, "s1", MIN), "Grep")),
        vec![Persist]
    );
    assert_eq!(h.state(), Some(State::Working));
    for (e, reason) in [
        (request("s1", MIN), Reason::NeedsPermission),
        (ev(Kind::NeedsAnswer, "s1", MIN), Reason::NeedsAnswer),
    ] {
        let mut h = H::into(State::Failed, 0);
        assert_eq!(h.send(e), vec![notify(reason), Persist]);
        assert_eq!(h.state(), Some(State::NeedsInput));
        assert_eq!(h.tick(6 * MIN), vec![escalate(), Persist]);
    }
}

// ADR 0018 item 5: ended_unexpectedly + any agent event → the state that
// event implies, as a first event, keeping the session's animal.
#[test]
fn an_event_after_an_unexpected_end_continues_the_session() {
    let animals = Settings {
        animals: vec!["fox".into(), "owl".into()],
        ..Settings::default()
    };
    use Reason::*;
    for (kind, state, fx) in [
        (Kind::SessionStarted, State::Idle, vec![Persist]),
        (Kind::TurnIdle, State::Idle, vec![Persist]),
        (Kind::TurnStarted, State::Working, vec![Persist]),
        (Kind::Activity, State::Working, vec![Persist]),
        (
            Kind::NeedsPermission,
            State::NeedsInput,
            vec![notify(NeedsPermission), Persist],
        ),
        (
            Kind::NeedsAnswer,
            State::NeedsInput,
            vec![notify(NeedsAnswer), Persist],
        ),
        (
            Kind::TurnCompleted,
            State::Ready,
            vec![ready_add(), Persist],
        ),
        (
            Kind::AgentFailed,
            State::Failed,
            vec![notify(Failed), Persist],
        ),
    ] {
        let mut h = H::with(animals.clone());
        h.send(ev(Kind::TurnStarted, "s0", 0));
        h.send(request("s1", 0));
        h.kind(Kind::UserAck, 0);
        h.gone(MIN);
        assert_eq!(h.s1().animal, "owl");
        let mut e = ev(kind, "s1", 2 * MIN);
        e.context.cwd = r"C:\code\elsewhere".into();
        e.context.project = "elsewhere".into();
        assert_eq!(h.send(e), fx, "{kind:?}");
        let s = h.s1();
        assert_eq!((s.state, s.since, s.last_event), (state, 2 * MIN, 2 * MIN));
        assert_eq!(
            (s.animal.as_str(), s.project.as_str()),
            ("owl", "elsewhere")
        );
        assert_eq!(s.project_key, project_key(r"C:\code\elsewhere"));
        if let Some(n) = &s.needs {
            assert!(!n.seen && !n.escalated, "a fresh episode");
        }
        let order: Vec<_> = h.0.sessions().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(order, ["s0", "s1"], "keeps its place");
        // The 60 s fade was for the ended state only.
        h.tick(4 * MIN);
        assert_eq!(h.state(), Some(state), "{kind:?}");
    }
    // A new process: the old ProcessGone no longer counts, and the pid on the
    // new event makes the process known.
    let mut h = H::into(State::EndedUnexpectedly, 0);
    h.send(pid(ev(Kind::TurnStarted, "s1", MIN)));
    h.send(pid(ev(Kind::TurnCompleted, "s1", 2 * MIN)));
    assert_eq!(h.tick(100 * 60 * MIN), vec![]);
    assert_eq!(h.state(), Some(State::Ready));
    // session.ended still removes it.
    let mut h = H::into(State::EndedUnexpectedly, 0);
    assert_eq!(h.kind(Kind::SessionEnded, 1), vec![Persist]);
    assert_eq!(h.state(), None);
}

// ADR 0018 item 6: suppressed Notify and Escalate are dropped, not deferred,
// including for the new ADR 0018 entries into Needs you.
#[test]
fn suppressed_knocks_from_the_new_rows_are_dropped() {
    for from in [State::Stalled, State::Failed, State::NeedsInput] {
        let mut h = H::into(from, 10 * MIN);
        h.kind(Kind::UserAck, 10 * MIN);
        h.suppress(true);
        assert_eq!(h.send(request("s1", 11 * MIN)), vec![Persist], "{from:?}");
        assert_eq!(h.state(), Some(State::NeedsInput));
        assert_eq!(h.tick(16 * MIN), vec![Persist], "escalation dropped");
        h.suppress(false);
        assert_eq!(h.tick(17 * MIN), vec![], "{from:?}: not sent later");
        assert_eq!(h.tick(60 * MIN), vec![], "{from:?}: not sent later");
    }
}

/// ADR 0018 item 8: every pair with no §8.4 row leaves the session unchanged.
#[test]
fn pairs_without_a_row_change_nothing() {
    use State::*;
    for (from, kind) in [
        (Failed, Kind::TurnCompleted),
        (Failed, Kind::TurnIdle),
        (Failed, Kind::SessionStarted),
        (Stalled, Kind::TurnIdle),
        (Idle, Kind::TurnIdle),
        (NeedsInput, Kind::TurnIdle),
        (EndedUnexpectedly, Kind::UserAck),
        (Working, Kind::TurnStarted),
        (Idle, Kind::TurnCompleted),
        (Ready, Kind::TurnCompleted),
        (Working, Kind::TurnIdle),
        (Working, Kind::UserAck),
        (Stalled, Kind::UserAck),
        (Failed, Kind::UserAck),
        (Working, Kind::SessionStarted),
    ] {
        let mut h = H::into(from, 10 * MIN);
        let e = match kind {
            Kind::NeedsPermission => request("s1", 11 * MIN),
            k => ev(k, "s1", 11 * MIN),
        };
        assert_eq!(h.send(e), vec![], "{from:?} + {kind:?}");
        assert_eq!(h.state(), Some(from), "{from:?} + {kind:?}");
    }
}

// ADR 0019 item 2: a background SubagentStop (activity with an agentId)
// after a StopFailure leaves Failed as it is.
#[test]
fn background_activity_leaves_failed() {
    let mut h = H::into(State::Failed, 0);
    for (a, t) in [("a1", 1600), ("a2", 5000)] {
        assert_eq!(h.send(agent(ev(Kind::Activity, "s1", t), a)), vec![]);
        assert_eq!((h.state(), h.s1().since), (Some(State::Failed), 0));
    }
    assert_eq!(h.tick(30 * MIN), vec![], "doesn't stall either");
    assert_eq!(h.state(), Some(State::Failed));
}

// ADR 0019 item 1, the reviewer's first case: sub-agent A asks, then B.
// Approving A leaves B waiting; B's `permission_prompt` doesn't drop it.
#[test]
fn two_sub_agents_waiting_need_both_answered() {
    for prompt_after in [6000, 30_000] {
        let mut h = H::into(State::Working, 0);
        assert_eq!(
            h.send(agent(request("s1", 1000), "A")),
            vec![notify(Reason::NeedsPermission), Persist]
        );
        assert_eq!(
            h.send(agent(request("s1", 2000), "B")),
            vec![Persist],
            "no second Notify"
        );
        assert_eq!(h.waiting(), [Some("A"), Some("B")]);
        // A is approved.
        assert_eq!(
            h.send(agent(ev(Kind::Activity, "s1", 3000), "A")),
            vec![Persist]
        );
        assert_eq!(
            (h.state(), h.waiting()),
            (Some(State::NeedsInput), vec![Some("B")])
        );
        // B's prompt: within 10 s it's B's request again, later it is B
        // still waiting. Either way B stays.
        assert_eq!(
            h.send(agent(prompt("s1", 2000 + prompt_after), "B")),
            vec![]
        );
        assert_eq!(
            (h.state(), h.waiting()),
            (Some(State::NeedsInput), vec![Some("B")])
        );
        // B still knocks: the episode escalates.
        assert_eq!(h.tick(1000 + 5 * MIN), vec![escalate(), Persist]);
        // B is answered: now it closes.
        assert_eq!(
            h.send(tool(agent(ev(Kind::Activity, "s1", 6 * MIN), "B"), "Bash")),
            vec![Persist]
        );
        assert_eq!(
            (h.state(), h.s1().activity),
            (Some(State::Working), Some(Category::Run))
        );
    }
}

// ADR 0019 item 1, the reviewer's second case: a background request, then
// the main agent's Stop. The session stays needs_input.
#[test]
fn the_main_stop_keeps_a_waiting_sub_agent() {
    let mut h = H::into(State::Working, 0);
    h.send(agent(request("s1", 1000), "A"));
    assert_eq!(
        h.kind(Kind::TurnCompleted, 2000),
        vec![],
        "no ReadyBatchAdd"
    );
    assert_eq!(
        (h.state(), h.waiting()),
        (Some(State::NeedsInput), vec![Some("A")])
    );
    assert_eq!(h.tick(1000 + 5 * MIN), vec![escalate(), Persist]);
    // A is answered: the main turn is done, so it closes to ready (ADR 0021).
    assert_eq!(
        h.send(agent(ev(Kind::Activity, "s1", 6 * MIN), "A")),
        vec![ready_add(), Persist]
    );
    assert_eq!(h.state(), Some(State::Ready));
    assert_eq!(h.tick(17 * MIN), vec![], "no stall");
    // Main and A both waiting: the Stop removes the main agent only.
    let mut h = H::into(State::Working, 0);
    h.send(request("s1", 1000));
    h.send(agent(request("s1", 2000), "A"));
    assert_eq!(h.kind(Kind::TurnCompleted, 3000), vec![Persist]);
    assert_eq!(
        (h.state(), h.waiting()),
        (Some(State::NeedsInput), vec![Some("A")])
    );
    // Only the main agent waiting: ready, as before.
    let mut h = H::into(State::Working, 0);
    h.send(request("s1", 1000));
    h.send(request("s1", 2000));
    assert_eq!(
        h.kind(Kind::TurnCompleted, 3000),
        vec![ready_add(), Persist]
    );
    // turn.started closes the whole episode.
    let mut h = H::into(State::Working, 0);
    h.send(request("s1", 1000));
    h.send(agent(request("s1", 2000), "A"));
    assert_eq!(h.kind(Kind::TurnStarted, 3000), vec![Persist]);
    assert_eq!(
        (h.state(), h.s1().needs.clone()),
        (Some(State::Working), None)
    );
}

// ADR 0019: a new requester joins the open episode; after a click a new
// request opens a new unseen episode that still waits on the old requesters.
#[test]
fn requesters_join_and_carry_into_a_new_episode() {
    let mut h = H::into(State::NeedsInput, 0);
    assert_eq!(
        h.send(agent(ev(Kind::NeedsAnswer, "s1", 1000), "A")),
        vec![Persist]
    );
    assert_eq!(
        h.send(agent(ev(Kind::NeedsAnswer, "s1", 2000), "A")),
        vec![],
        "already waiting"
    );
    assert_eq!(h.waiting(), [None, Some("A")]);
    assert_eq!(
        h.s1().needs.as_ref().unwrap().kind,
        NeedsKind::Permission,
        "kind is the opener's"
    );
    h.kind(Kind::UserAck, 3000);
    // A prompt (not a new request) from a new requester joins the seen episode.
    assert_eq!(h.send(agent(prompt("s1", 4000), "B")), vec![Persist]);
    assert!(h.s1().needs.as_ref().unwrap().seen);
    assert_eq!(
        h.send(agent(request("s1", 5000), "C")),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    let n = h.s1().needs.clone().unwrap();
    assert_eq!((n.seen, n.opened), (false, 5000));
    assert_eq!(h.waiting(), [None, Some("A"), Some("B"), Some("C")]);
    for (who, left) in [(Some("C"), 3), (None, 2), (Some("B"), 1)] {
        let e = ev(Kind::Activity, "s1", 6000);
        let e = match who {
            Some(a) => agent(e, a),
            None => e,
        };
        assert_eq!(h.send(e), vec![Persist]);
        assert_eq!(
            (h.state(), h.waiting().len()),
            (Some(State::NeedsInput), left)
        );
    }
    assert_eq!(
        h.send(agent(ev(Kind::Activity, "s1", 7000), "A")),
        vec![Persist]
    );
    assert_eq!(h.state(), Some(State::Working));
}

// ADR 0019 item 3: the 10 s dedup is per requester; a prompt without an
// agentId matches any requester (ADR 0021).
#[test]
fn dedup_pairs_a_prompt_with_its_own_requester_only() {
    // A's request was approved fast and the turn ended; A's late prompt is
    // still A's request, but B's isn't.
    let mut h = H::into(State::Working, 0);
    h.send(agent(request("s1", 1000), "A"));
    h.send(agent(ev(Kind::Activity, "s1", 2000), "A"));
    h.kind(Kind::TurnCompleted, 3000);
    assert_eq!(h.send(agent(prompt("s1", 7000), "A")), vec![]);
    assert_eq!(
        h.send(agent(prompt("s1", 7000), "B")),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    assert_eq!(h.waiting(), [Some("B")]);
    // C's prompt in the open episode joins it; A's (a duplicate) doesn't.
    assert_eq!(h.send(agent(prompt("s1", 8000), "A")), vec![]);
    assert_eq!(h.send(agent(prompt("s1", 8000), "C")), vec![Persist]);
    assert_eq!(h.waiting(), [Some("B"), Some("C")]);
    // Past 10 s, a prompt without agentId is the main agent's.
    assert_eq!(h.send(prompt("s1", 12_000)), vec![Persist]);
    assert_eq!(h.waiting(), [Some("B"), Some("C"), None]);
}

// ADR 0021 item 1: a sub-agent's request answered within 10 s, then its late
// prompt without agentId: still that request, no new episode.
#[test]
fn a_late_prompt_without_agent_id_matches_any_requester() {
    for t in [1000 + 1, 1000 + 10_000] {
        let mut h = H::into(State::Working, 0);
        assert_eq!(
            h.send(agent(request("s1", 1000), "A")),
            vec![notify(Reason::NeedsPermission), Persist]
        );
        assert_eq!(
            h.send(agent(ev(Kind::Activity, "s1", 1000), "A")),
            vec![Persist]
        );
        assert_eq!(h.send(prompt("s1", t)), vec![], "at {t}");
        assert_eq!(h.state(), Some(State::Working));
    }
    // 10 s and 1 ms later it's a new episode (the main agent's).
    let mut h = H::into(State::Working, 0);
    h.send(agent(request("s1", 1000), "A"));
    h.send(agent(ev(Kind::Activity, "s1", 1000), "A"));
    assert_eq!(
        h.send(prompt("s1", 11_001)),
        vec![notify(Reason::NeedsPermission), Persist]
    );
    assert_eq!(h.waiting(), [None]);
}

// ADR 0021 item 2: a background request after the main agent's Stop, then
// answered: ready with one ReadyBatchAdd, and no stall later. turn.started
// still closes to working; the main agent asking again means its turn isn't
// done, so the close is to working as before.
#[test]
fn a_background_request_after_the_stop_closes_to_ready() {
    // From ready, and from idle after the user acknowledged ready.
    for ack in [false, true] {
        let mut h = H::into(State::Ready, 0);
        if ack {
            h.kind(Kind::UserAck, 500);
        }
        assert_eq!(
            h.send(agent(request("s1", 1000), "A")),
            vec![notify(Reason::NeedsPermission), Persist]
        );
        assert_eq!(
            h.send(agent(ev(Kind::Activity, "s1", 4000), "A")),
            vec![ready_add(), Persist],
            "ack {ack}"
        );
        assert_eq!((h.state(), h.s1().since), (Some(State::Ready), 4000));
        assert_eq!(h.tick(4000 + 10 * MIN), vec![], "no stall");
        assert_eq!(h.tick(4000 + 60 * MIN), vec![]);
    }
    // turn.started still closes to working.
    let mut h = H::into(State::Ready, 0);
    h.send(agent(request("s1", 1000), "A"));
    assert_eq!(h.kind(Kind::TurnStarted, 2000), vec![Persist]);
    assert_eq!(h.state(), Some(State::Working));
    // The main agent asks too: answered last, it closes to working.
    let mut h = H::into(State::Ready, 0);
    h.send(agent(request("s1", 1000), "A"));
    h.send(request("s1", 2000));
    h.send(agent(ev(Kind::Activity, "s1", 3000), "A"));
    assert_eq!(h.kind(Kind::Activity, 4000), vec![Persist]);
    assert_eq!(h.state(), Some(State::Working));
    // A new turn after the close: the next background request closes to
    // working again.
    let mut h = H::into(State::Ready, 0);
    h.kind(Kind::TurnStarted, 1000);
    h.send(agent(request("s1", 2000), "A"));
    assert_eq!(
        h.send(agent(ev(Kind::Activity, "s1", 3000), "A")),
        vec![Persist]
    );
    assert_eq!(h.state(), Some(State::Working));
}

// Finding 4: suppression holds from the very first input, with no tick.
#[test]
fn suppression_set_before_the_first_event_holds_for_it() {
    for (k, state) in [
        (Kind::NeedsPermission, State::NeedsInput),
        (Kind::NeedsAnswer, State::NeedsInput),
        (Kind::AgentFailed, State::Failed),
    ] {
        let mut h = H::new();
        h.suppress(true);
        assert_eq!(h.kind(k, 500), vec![Persist], "{k:?}");
        assert_eq!(h.state(), Some(state));
        // Lifted between two events, without a tick: the next knock is sent.
        h.suppress(false);
        assert_eq!(h.kind(Kind::TurnStarted, 700), vec![Persist]);
        assert_eq!(h.send(request("s1", 900)).len(), 2, "{k:?}");
    }
}
