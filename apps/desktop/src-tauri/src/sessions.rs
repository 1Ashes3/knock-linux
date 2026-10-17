//! Spike C session store: what hook events told us about each session.
//! Holds only §11.4 fields: session id, project name, PID and start time,
//! window owner and precision. Replaced by the engine snapshot in Phase 1.

use crate::jump::{Precision, ProcessId, Target};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

struct Session {
    project: String,
    process: Option<ProcessId>,
    /// Executable that owns the session's window, file name only.
    owner: Option<String>,
    precision: Precision,
    /// Order of the latest event, for "most recent first".
    last: u64,
}

/// §8.5 `SessionView`. Spike C fills only these fields.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub project: String,
    pub precision: Precision,
}

#[derive(Default)]
pub struct Sessions(Mutex<(u64, HashMap<String, Session>)>);

impl Sessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, HashMap<String, Session>)> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn len(&self) -> usize {
        self.lock().1.len()
    }

    /// Whether the session's process is already known (§9.4: look up once).
    pub fn has_process(&self, id: &str) -> bool {
        self.lock().1.get(id).is_some_and(|s| s.process.is_some())
    }

    /// Records an event for `id`. `found` is the process and window from a
    /// lookup, if one was made; a known process is never replaced.
    pub fn record(&self, id: &str, project: &str, found: Option<(ProcessId, Option<Target>)>) {
        let mut g = self.lock();
        g.0 += 1;
        let last = g.0;
        let s = g.1.entry(id.to_owned()).or_insert_with(|| Session {
            project: project.to_owned(),
            process: None,
            owner: None,
            precision: Precision::None,
            last,
        });
        s.last = last;
        if s.process.is_none()
            && let Some((process, target)) = found
        {
            s.process = Some(process);
            s.owner = target.as_ref().map(|t| t.owner.clone());
            s.precision = target.map_or(Precision::None, |t| t.precision);
        }
    }

    /// Most recent event first.
    pub fn views(&self) -> Vec<SessionView> {
        let g = self.lock();
        let mut v: Vec<_> = g.1.iter().collect();
        v.sort_by_key(|(_, s)| std::cmp::Reverse(s.last));
        v.into_iter()
            .map(|(id, s)| SessionView {
                id: id.clone(),
                project: s.project.clone(),
                precision: s.precision,
            })
            .collect()
    }

    /// For a jump: the session's process, project and recorded window owner.
    pub fn jump_info(&self, id: &str) -> Option<(ProcessId, String, Option<String>)> {
        let g = self.lock();
        let s = g.1.get(id)?;
        Some((s.process?, s.project.clone(), s.owner.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(precision: Precision) -> Target {
        Target {
            window: 1,
            owner: "WindowsTerminal.exe".into(),
            precision,
        }
    }

    #[test]
    fn most_recent_first_and_process_recorded_once() {
        let s = Sessions::default();
        let p1 = ProcessId { pid: 10, start: 1 };
        s.record("a", "alpha", Some((p1, Some(target(Precision::AppWindow)))));
        s.record("b", "beta", None);
        assert!(s.has_process("a") && !s.has_process("b"));
        let ids = |s: &Sessions| s.views().into_iter().map(|v| v.id).collect::<Vec<_>>();
        assert_eq!(ids(&s), ["b", "a"]);
        // A later lookup never replaces the first one.
        let p2 = ProcessId { pid: 20, start: 2 };
        s.record("a", "alpha", Some((p2, None)));
        assert_eq!(ids(&s), ["a", "b"]);
        let (p, project, owner) = s.jump_info("a").unwrap();
        assert_eq!((p, project.as_str()), (p1, "alpha"));
        assert_eq!(owner.as_deref(), Some("WindowsTerminal.exe"));
        assert_eq!(s.views()[0].precision, Precision::AppWindow);
        // No process yet: no jump info, precision none.
        assert!(s.jump_info("b").is_none());
        assert!(s.jump_info("zzz").is_none());
        assert_eq!(s.views()[1].precision, Precision::None);
        // Found later (first lookup failed): recorded then.
        s.record("b", "beta", Some((p2, None)));
        assert!(s.has_process("b"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn view_uses_spec_field_names() {
        let v = SessionView {
            id: "s".into(),
            project: "p".into(),
            precision: Precision::ExactWindow,
        };
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"id":"s","project":"p","precision":"exact_window"}"#
        );
    }
}
