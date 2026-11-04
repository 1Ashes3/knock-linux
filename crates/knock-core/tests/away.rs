//! FR-9 away detection (§9.7 Idle time, ADR 0034) on a virtual clock: the
//! engine's tick, then the away tick, once a second, with the idle time the
//! app would read from the OS.

use knock_core::{
    AWAY_MS, Away, AwaySummary, Category, Context, Detail, Engine, Event, Input, Kind, Outcome,
    Settings, summary,
};

const MIN: u64 = 60_000;
const T0: u64 = 1_792_000_000_000;

fn ev(kind: Kind, sid: &str, ts: u64) -> Event {
    Event {
        v: 1,
        agent: "claude-code",
        session_id: sid.into(),
        ts,
        kind,
        detail: Detail::default(),
        context: Context {
            cwd: format!(r"C:\code\repo-{sid}"),
            project: format!("repo-{sid}"),
            pid: None,
            agent_id: None,
        },
    }
}

fn permission(sid: &str, ts: u64) -> Event {
    let mut e = ev(Kind::NeedsPermission, sid, ts);
    e.detail.tool = Some("Bash".into());
    e.detail.category = Some(Category::Run);
    e
}

/// The engine and the away tracker, ticked together. Times are ms after T0.
struct H {
    engine: Engine,
    away: Away,
    /// Last user input.
    input: u64,
    /// Next tick.
    clock: u64,
    /// Every summary, with the tick it came on.
    got: Vec<(u64, AwaySummary)>,
}

impl H {
    fn new() -> Self {
        H {
            engine: Engine::new(Settings::default()),
            away: Away::new(AWAY_MS),
            input: 0,
            clock: 0,
            got: Vec::new(),
        }
    }

    /// Ticks every second up to and including `t`.
    fn until(&mut self, t: u64) -> &mut Self {
        while self.clock <= t {
            let now = T0 + self.clock;
            self.engine.step(Input::Tick { now });
            let idle = self.clock.saturating_sub(self.input);
            if let Some(s) = self.away.tick(now, idle, self.engine.sessions()) {
                self.got.push((self.clock, s));
            }
            self.clock += 1000;
        }
        self
    }

    /// The user touches the mouse or keyboard at `t`.
    fn touch(&mut self, t: u64) -> &mut Self {
        self.until(t.saturating_sub(1));
        self.input = t;
        self
    }

    fn send(&mut self, kind: Kind, sid: &str, t: u64) -> &mut Self {
        self.event(ev(kind, sid, T0 + t), false)
    }

    fn event(&mut self, e: Event, simulated: bool) -> &mut Self {
        let t = e.ts - T0;
        self.until(t.saturating_sub(1));
        self.engine.step(Input::Event {
            event: e,
            simulated,
        });
        self
    }

    fn ask(&mut self, sid: &str, t: u64) -> &mut Self {
        self.event(permission(sid, T0 + t), false)
    }

    /// Items of the one summary, as (id, outcome).
    fn items(&self) -> Vec<(String, &'static str)> {
        assert_eq!(self.got.len(), 1, "{:?}", self.got);
        let s = &self.got[0].1;
        s.items
            .iter()
            .map(|i| (i.id.clone(), i.outcome.as_str()))
            .collect()
    }
}

fn pairs(v: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    v.iter().map(|(a, b)| (a.to_string(), *b)).collect()
}

#[test]
fn under_ten_minutes_never_counts() {
    let mut h = H::new();
    h.touch(1000);
    h.send(Kind::TurnStarted, "a", 2000);
    h.send(Kind::TurnCompleted, "a", 3000);
    // Idle reaches 9:59 (ticks are whole seconds; the last one before the
    // input is 1000 + 599 s), then input.
    h.until(1000 + AWAY_MS - 1000);
    assert_eq!(h.away.since(), None);
    h.touch(1000 + AWAY_MS - 500).until(1000 + AWAY_MS + 5000);
    assert!(h.got.is_empty());
    assert_eq!(h.away.since(), None);
}

#[test]
fn exactly_ten_minutes_counts_and_since_is_the_last_input() {
    let mut h = H::new();
    h.touch(5_000);
    h.send(Kind::TurnStarted, "a", 6_000);
    h.until(5_000 + AWAY_MS - 1000);
    assert_eq!(h.away.since(), None, "9:59");
    h.until(5_000 + AWAY_MS);
    assert_eq!(h.away.since(), Some(T0 + 5_000), "10:00, since the input");
    h.send(Kind::TurnCompleted, "a", 11 * MIN);
    h.until(20 * MIN);
    assert!(h.got.is_empty(), "nothing until the user returns");
    h.touch(20 * MIN + 300).until(20 * MIN + 2000);
    assert_eq!(h.got[0].0, 20 * MIN + 1000, "on the first tick after input");
    assert_eq!(h.got[0].1.since, T0 + 5_000);
    assert_eq!(h.items(), pairs(&[("a", "ready")]));
    assert_eq!(h.away.since(), None);
}

#[test]
fn a_shorter_threshold_for_manual_tests() {
    let mut h = H::new();
    h.away = Away::new(30_000);
    h.touch(1000);
    h.send(Kind::TurnStarted, "a", 2000);
    h.send(Kind::TurnCompleted, "a", 10_000);
    h.until(31_000);
    assert_eq!(h.away.since(), Some(T0 + 1000));
    h.touch(40_000).until(41_000);
    assert_eq!(h.items(), pairs(&[("a", "ready")]));
}

/// Every ADR 0034 inclusion and exclusion case in one idle period from
/// 1 s to 30 min.
#[test]
fn adr_0034_inclusions_and_exclusions() {
    let mut h = H::new();
    // Before the idle period.
    h.send(Kind::TurnStarted, "entered-stays", 0);
    h.send(Kind::TurnStarted, "entered-left", 0);
    h.send(Kind::TurnCompleted, "before-never-left", 0);
    h.send(Kind::TurnStarted, "removed", 0);
    h.send(Kind::TurnStarted, "stalls", 0);
    h.send(Kind::AgentFailed, "before-left-back", 0);
    h.send(Kind::TurnCompleted, "before-to-other", 0);
    h.ask("needs", 0);
    h.send(Kind::TurnStarted, "needs", 100);
    h.send(Kind::TurnStarted, "fails", 100);
    h.send(Kind::TurnStarted, "still-working", 100);
    h.touch(1000);
    // During it.
    h.send(Kind::TurnCompleted, "entered-stays", 2 * MIN);
    h.send(Kind::TurnCompleted, "entered-left", 2 * MIN);
    h.send(Kind::TurnStarted, "entered-left", 3 * MIN);
    h.send(Kind::TurnCompleted, "removed", 2 * MIN);
    h.send(Kind::SessionEnded, "removed", 4 * MIN);
    h.send(Kind::TurnStarted, "new", 5 * MIN);
    h.send(Kind::TurnCompleted, "new", 6 * MIN);
    h.send(Kind::TurnStarted, "before-left-back", 6 * MIN);
    h.send(Kind::AgentFailed, "before-left-back", 7 * MIN);
    h.ask("before-to-other", 7 * MIN);
    h.ask("needs", 8 * MIN);
    h.send(Kind::AgentFailed, "fails", 9 * MIN);
    // `still-working` keeps working; `entered-left` too.
    for t in (10..30).map(|m| m * MIN) {
        h.send(Kind::Activity, "still-working", t);
        h.send(Kind::Activity, "entered-left", t);
    }
    // `stalls` stalls at 10 min after its last event (0 s).
    h.touch(30 * MIN).until(30 * MIN + 1000);
    assert_eq!(h.got[0].1.since, T0 + 1000);
    assert_eq!(
        h.items(),
        pairs(&[
            ("entered-stays", "ready"),
            ("stalls", "needs_you"),
            ("before-left-back", "failed"),
            ("before-to-other", "needs_you"),
            ("needs", "needs_you"),
            ("fails", "failed"),
            ("new", "ready"),
        ])
    );
}

/// ADR 0034 exception: seen before the user left, then a new request
/// during the idle period opens a new episode (ADR 0016, 0018 item 1). The
/// state never left `needs_input`, but the session is listed.
#[test]
fn a_seen_needs_input_that_asks_again_while_away_is_listed() {
    let mut h = H::new();
    h.ask("reasked", 0);
    h.send(Kind::UserAck, "reasked", 500);
    h.touch(1000);
    h.ask("reasked", 5 * MIN);
    let s = &h.engine.sessions()[0];
    let n = s.needs.as_ref().unwrap();
    assert_eq!((s.since, n.seen, n.opened), (T0, false, T0 + 5 * MIN));
    h.touch(15 * MIN).until(15 * MIN + 1000);
    assert_eq!(h.got[0].1.since, T0 + 1000);
    assert_eq!(h.items(), pairs(&[("reasked", "needs_you")]));
}

/// Still excluded: a seen episode from before with no new request, and an
/// episode from before that a prompt or another requester only joined.
#[test]
fn a_needs_input_episode_opened_before_the_idle_period_is_not_listed() {
    let mut h = H::new();
    h.ask("seen", 0);
    h.send(Kind::UserAck, "seen", 500);
    h.ask("prompt-joins", 0);
    h.send(Kind::UserAck, "prompt-joins", 500);
    h.ask("sub-joins", 0);
    h.touch(1000);
    // The `permission_prompt` Notification (no tool) after the dedup window.
    h.send(Kind::NeedsPermission, "prompt-joins", 5 * MIN);
    // A sub-agent asks while the main agent's episode is still unseen.
    let mut sub = permission("sub-joins", T0 + 5 * MIN);
    sub.context.agent_id = Some("agent-1".into());
    h.event(sub, false);
    let opened: Vec<_> = (h.engine.sessions().iter())
        .map(|s| s.needs.as_ref().unwrap().opened)
        .collect();
    assert_eq!(opened, [T0, T0, T0], "no new episode");
    h.touch(15 * MIN).until(15 * MIN + 1000);
    assert!(h.got.is_empty(), "{:?}", h.got);
}

#[test]
fn simulated_sessions_count_like_real_ones() {
    let mut h = H::new();
    h.touch(1000);
    let mut e = ev(Kind::TurnCompleted, "sim", T0 + 2 * MIN);
    e.context.project = "web-shop".into();
    h.event(e, true);
    h.touch(15 * MIN).until(15 * MIN + 1000);
    let s = &h.got[0].1;
    assert_eq!(s.items.len(), 1);
    assert_eq!(
        (s.items[0].id.as_str(), s.items[0].project.as_str()),
        ("sim", "web-shop")
    );
    assert!(h.engine.sessions()[0].simulated);
}

#[test]
fn no_summary_when_nothing_qualifies() {
    let mut h = H::new();
    h.send(Kind::TurnCompleted, "ready-before", 0);
    h.send(Kind::TurnStarted, "working", 500);
    h.touch(1000);
    for t in (1..20).map(|m| m * MIN) {
        h.send(Kind::Activity, "working", t);
    }
    h.touch(20 * MIN).until(21 * MIN);
    assert!(h.got.is_empty());
    assert_eq!(h.away.since(), None, "back, not away");
    // No sessions at all.
    let mut h = H::new();
    h.touch(1000).until(15 * MIN);
    h.touch(15 * MIN).until(16 * MIN);
    assert!(h.got.is_empty());
}

#[test]
fn each_idle_period_gets_its_own_summary() {
    let mut h = H::new();
    h.touch(1000);
    h.send(Kind::TurnStarted, "a", 2000);
    h.send(Kind::TurnCompleted, "a", 3 * MIN);
    h.touch(12 * MIN).until(12 * MIN + 1000);
    assert_eq!(h.got.len(), 1);
    // Back: `a` is still ready, but that was before the next idle period.
    h.send(Kind::TurnStarted, "b", 12 * MIN + 2000);
    h.send(Kind::TurnCompleted, "b", 15 * MIN);
    h.touch(25 * MIN).until(25 * MIN + 1000);
    assert_eq!(h.got.len(), 2);
    let second = &h.got[1].1;
    assert_eq!(second.since, T0 + 12 * MIN);
    let ids: Vec<_> = second.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["b"]);
}

/// Ticks stopped for an hour (the PC slept). The idle time is lower than at
/// the last tick, so input was seen: that is the return. Here the user also
/// left again more than 10 minutes ago, so a new idle period starts at once.
#[test]
fn a_return_and_a_new_idle_period_between_two_ticks() {
    let mut engine = Engine::new(Settings::default());
    engine.step(Input::Event {
        event: ev(Kind::TurnCompleted, "a", T0 + 2 * MIN),
        simulated: false,
    });
    let mut away = Away::new(AWAY_MS);
    assert_eq!(away.tick(T0 + 20 * MIN, 20 * MIN, engine.sessions()), None);
    assert_eq!(away.since(), Some(T0));
    let got = away.tick(T0 + 80 * MIN, 12 * MIN, engine.sessions());
    assert_eq!(got.unwrap().since, T0);
    assert_eq!(away.since(), Some(T0 + 68 * MIN));
    // Woken by input a second ago: back, not away.
    let mut away = Away::new(AWAY_MS);
    away.tick(T0 + 20 * MIN, 20 * MIN, engine.sessions());
    assert!(away.tick(T0 + 80 * MIN, 1000, engine.sessions()).is_some());
    assert_eq!(away.since(), None);
}

#[test]
fn outcomes_and_the_summary_function() {
    use knock_core::State;
    assert_eq!(Outcome::of(State::Ready), Some(Outcome::Ready));
    assert_eq!(Outcome::of(State::Failed), Some(Outcome::Failed));
    assert_eq!(Outcome::of(State::NeedsInput), Some(Outcome::NeedsYou));
    assert_eq!(Outcome::of(State::Stalled), Some(Outcome::NeedsYou));
    for s in [
        State::Working,
        State::Idle,
        State::EndedUnexpectedly,
        State::BlockedFixable,
        State::WaitingAgent,
        State::Communicating,
        State::Paused,
        State::Cancelled,
    ] {
        assert_eq!(Outcome::of(s), None, "{s:?}");
    }
    let strs = [Outcome::Ready, Outcome::Failed, Outcome::NeedsYou].map(Outcome::as_str);
    assert_eq!(strs, ["ready", "failed", "needs_you"]);
    assert_eq!(summary(T0, &[]), None);
}
