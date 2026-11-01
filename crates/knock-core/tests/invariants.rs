//! §12.1 Invariants: the §8.4 invariants (with ADR 0016's) over random
//! input sequences, plus the ADR 0019 requester rules: no waiting requester
//! is ever lost, and background activity never clears Failed.

use knock_core::{
    Context, Detail, Effect, Engine, Event, Input, Kind, Reason, Session, Settings, State, Tier,
};
use proptest::prelude::*;
use std::collections::HashMap;

const KINDS: [Kind; 11] = [
    Kind::SessionStarted,
    Kind::TurnStarted,
    Kind::Activity,
    Kind::NeedsPermission,
    Kind::NeedsAnswer,
    Kind::TurnIdle,
    Kind::TurnCompleted,
    Kind::AgentFailed,
    Kind::SessionEnded,
    Kind::UserAck,
    Kind::UserDismiss,
];

#[derive(Debug, Clone)]
enum Op {
    /// session, kind, agent (0 = main), has a tool, ms later.
    Event(u8, usize, u8, bool, u64),
    /// ms later.
    Tick(u64),
    Suppress(bool),
    Gone(u8, u64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0..3u8, 0..KINDS.len(), 0..3u8, any::<bool>(), 0..12_000u64)
            .prop_map(|(s, k, a, t, d)| Op::Event(s, k, a, t, d)),
        3 => prop_oneof![0..3_000u64, 0..400_000u64].prop_map(Op::Tick),
        1 => any::<bool>().prop_map(Op::Suppress),
        1 => (0..3u8, 0..3_000u64).prop_map(|(s, d)| Op::Gone(s, d)),
    ]
}

fn inputs(ops: &[Op]) -> Vec<Input> {
    let mut now = 0;
    ops.iter()
        .map(|op| match *op {
            Op::Event(s, k, a, t, d) => {
                now += d;
                Input::Event {
                    event: Event {
                        v: 1,
                        agent: "claude-code",
                        session_id: format!("s{s}"),
                        ts: now,
                        kind: KINDS[k],
                        detail: Detail {
                            tool: t.then(|| "Write".into()),
                            ..Detail::default()
                        },
                        context: Context {
                            cwd: format!(r"C:\code\p{s}"),
                            project: format!("p{s}"),
                            pid: None,
                            agent_id: (a > 0).then(|| format!("a{a}")),
                        },
                    },
                    simulated: false,
                }
            }
            Op::Tick(d) => {
                now += d;
                Input::Tick { now }
            }
            Op::Suppress(q) => Input::SetSuppressed(q),
            Op::Gone(s, d) => {
                now += d;
                Input::ProcessGone {
                    session_id: format!("s{s}"),
                    now,
                }
            }
        })
        .collect()
}

fn settings() -> impl Strategy<Value = Settings> {
    (1..6u64, 1..6u64, any::<bool>()).prop_map(|(st, esc, n)| Settings {
        stall_ms: st * 60_000,
        escalation_ms: esc * 60_000,
        notify_on_stall: n,
        ..Settings::default()
    })
}

/// §8.4 tier table for the states v1 produces.
fn tier_of(s: State) -> Option<Tier> {
    use State::*;
    Some(match s {
        NeedsInput | Failed | Stalled => Tier::NeedsYou,
        Ready => Tier::Ready,
        Working => Tier::Working,
        Idle | EndedUnexpectedly => Tier::Resting,
        _ => return None,
    })
}

fn sid(e: &Effect) -> Option<&str> {
    match e {
        Effect::Notify { session_id, .. }
        | Effect::Escalate { session_id }
        | Effect::ReadyBatchAdd { session_id } => Some(session_id),
        Effect::Persist => None,
    }
}

#[derive(Default)]
struct Episode {
    notify: u32,
    escalate: u32,
    seen: bool,
}

fn run(settings: &Settings, inputs: &[Input]) -> (Vec<Vec<Effect>>, Vec<Session>) {
    let mut engine = Engine::new(settings.clone());
    let out = inputs.iter().map(|i| engine.step(i.clone())).collect();
    (out, engine.sessions().to_vec())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn invariants_hold(settings in settings(), ops in prop::collection::vec(op(), 1..150)) {
        let inputs = inputs(&ops);
        let mut engine = Engine::new(settings.clone());
        // State, and for needs_input the episode's (seen, opened, waiting).
        type Prev = (State, Option<(bool, u64, Vec<Option<String>>)>);
        let mut prev: HashMap<String, Prev> = HashMap::new();
        let now = |s: &Session| (s.state, s.needs.as_ref().map(|n| (n.seen, n.opened, n.waiting().to_vec())));
        let mut ep: HashMap<String, u32> = HashMap::new();
        let mut episodes: HashMap<(String, u32), Episode> = HashMap::new();
        let mut suppressed = false;
        for input in &inputs {
            if let Input::SetSuppressed(q) = input {
                suppressed = *q;
            }
            let event = match input {
                Input::Event { event, .. } => Some(event),
                _ => None,
            };
            let fx = engine.step(input.clone());
            let mut entered = Vec::new();
            for s in engine.sessions() {
                // 1. Exactly one state and one tier, as in the §8.4 table.
                prop_assert_eq!(tier_of(s.state), Some(s.state.tier()), "{:?}", s.state);
                // 2. needs_input always has needs.kind (and only needs_input has needs).
                prop_assert_eq!(s.state == State::NeedsInput, s.needs.is_some());
                let before = prev.get(&s.id).cloned();
                // A new episode: entering Needs you, or a new request after
                // the open one was seen (ADR 0018). Unseen, it stays the same.
                let reopened = match (&before, now(s)) {
                    (Some((State::NeedsInput, Some((seen, opened, _)))), (State::NeedsInput, Some(n))) => {
                        prop_assert!(*seen || n.1 == *opened, "unseen episode replaced");
                        *seen && !n.0
                    }
                    _ => false,
                };
                // ADR 0019 requester rules for this input's event, if it is
                // for this session.
                let mine = event.filter(|e| e.session_id == s.id);
                let who = mine.map(|e| e.context.agent_id.clone());
                let kind = mine.map(|e| e.kind);
                if let Some(n) = &s.needs {
                    prop_assert!(!n.waiting().is_empty(), "an episode with no requester");
                    // A request is always waited on after it arrives.
                    let request = matches!(kind, Some(Kind::NeedsAnswer))
                        || matches!(mine, Some(e) if e.kind == Kind::NeedsPermission && e.detail.tool.is_some());
                    if request {
                        prop_assert!(n.waiting().contains(who.as_ref().unwrap()), "request not waited on");
                    }
                }
                if let Some((State::NeedsInput, Some((_, _, was)))) = &before {
                    // Only that requester's activity, or the main agent's Stop,
                    // removes one.
                    let removed = match kind {
                        Some(Kind::Activity) => who.clone(),
                        Some(Kind::TurnCompleted) => Some(None),
                        _ => None,
                    };
                    let left: Vec<_> = was.iter().filter(|a| Some(*a) != removed.as_ref()).collect();
                    match &s.needs {
                        Some(n) => {
                            for a in &left {
                                prop_assert!(n.waiting().contains(a), "requester {:?} lost", a);
                            }
                            prop_assert!(!left.is_empty() || removed.is_none(), "kept open with none left");
                        }
                        None => {
                            // Left Needs you: none was left, or the user typed,
                            // it failed, or its process went.
                            let closes_all = s.state == State::EndedUnexpectedly
                                || matches!(kind, Some(Kind::TurnStarted | Kind::AgentFailed));
                            prop_assert!(left.is_empty() || closes_all, "{:?} with {:?} waiting", s.state, left);
                        }
                    }
                }
                if let (Some((State::Failed, _)), Some(Kind::Activity), Some(Some(_))) = (&before, kind, &who) {
                    prop_assert_eq!(s.state, State::Failed, "background activity cleared Failed");
                }
                if reopened || (before.as_ref().map(|p| p.0) != Some(s.state) && s.state.tier() == Tier::NeedsYou) {
                    *ep.entry(s.id.clone()).or_default() += 1;
                    entered.push((s.id.clone(), s.state));
                }
                let e = episodes.entry((s.id.clone(), ep.get(&s.id).copied().unwrap_or(0))).or_default();
                e.seen |= s.needs.as_ref().is_some_and(|n| n.seen);
            }
            for f in &fx {
                let Some(id) = sid(f) else { continue };
                let s = engine.sessions().iter().find(|s| s.id == id);
                let s = s.expect("effect for a session that doesn't exist");
                let e = episodes.entry((id.to_string(), ep.get(id).copied().unwrap_or(0))).or_default();
                match f {
                    Effect::Notify { .. } => {
                        // 4. Never a Notify for ready (or anything outside Needs you).
                        prop_assert_eq!(s.state.tier(), Tier::NeedsYou);
                        prop_assert!(!suppressed, "Notify while suppressed");
                        // 3. At most one Notify per episode.
                        e.notify += 1;
                        prop_assert!(e.notify <= 1, "second Notify in an episode");
                    }
                    Effect::Escalate { .. } => {
                        prop_assert_eq!(s.state, State::NeedsInput);
                        prop_assert!(!suppressed, "Escalate while suppressed");
                        // 3. At most one Escalate per episode, none once seen.
                        e.escalate += 1;
                        prop_assert!(e.escalate <= 1, "second Escalate in an episode");
                        prop_assert!(!e.seen, "Escalate after the episode was seen");
                    }
                    _ => {}
                }
            }
            // Zero missed Needs-you: entering it notifies unless suppressed
            // (or a stall with stall notifications off).
            for (id, state) in entered {
                let want = !suppressed && (state != State::Stalled || settings.notify_on_stall);
                let got = fx.iter().any(|f| matches!(f, Effect::Notify { session_id, reason }
                    if *session_id == id && (*reason == Reason::Stalled) == (state == State::Stalled)));
                prop_assert_eq!(got, want, "{} entered {:?}", id, state);
            }
            prev = engine.sessions().iter().map(|s| (s.id.clone(), now(s))).collect();
        }
    }

    // 5. The same inputs always give the same outputs.
    #[test]
    fn replay_is_deterministic(settings in settings(), ops in prop::collection::vec(op(), 1..150)) {
        let inputs = inputs(&ops);
        prop_assert_eq!(run(&settings, &inputs), run(&settings, &inputs));
    }
}
