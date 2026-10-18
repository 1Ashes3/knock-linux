//! PHASE 0 ONLY: the V-12 spike (SPEC.md §9.7, §10.2). Debug builds only;
//! replaced by the engine's notification effect in Phase 2.
//!
//! One toast per `PermissionRequest`, at most once per 10 s per session (a
//! crude stand-in for the engine). Text is §10.2's permission case: the
//! project and tool name only, nothing else from the payload. A click on the
//! toast jumps to the session, like `jump_to_session`.

use crate::platform::Platform;
use crate::sessions::Sessions;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const MIN_GAP: Duration = Duration::from_secs(10);

struct Spike {
    app: AppHandle,
    last: Mutex<HashMap<String, Instant>>,
}

// ponytail: a global so the server worker needs no Tauri handle; goes with the spike.
static SPIKE: OnceLock<Spike> = OnceLock::new();

/// Call once, after the platform and sessions are managed.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    // Runs on the plugin's waiting thread when the toast is clicked.
    let _ = app.notification().on_action(move |a| {
        let session = a
            .notification()
            .and_then(|n| n.extra().get("session"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        tracing::info!(action = ?a.action_id(), has_session = session.is_some(), "toast action");
        let (Some(id), "tap") = (session, a.action_id()) else {
            return;
        };
        let h = handle.clone();
        // On the main thread, like the `jump_to_session` command.
        let _ = handle.run_on_main_thread(move || {
            let sessions = h.state::<Arc<Sessions>>();
            let platform = h.state::<Arc<dyn Platform>>();
            crate::jump(&sessions, &**platform, &id, "toast");
        });
    });
    let _ = SPIKE.set(Spike {
        app: app.clone(),
        last: Mutex::default(),
    });
}

/// Shows the permission toast for `session`, unless one was shown in the last
/// 10 s. A no-op before `init` (as in server tests).
pub fn permission_request(session: &str, project: &str, tool: Option<&str>) {
    let Some(s) = SPIKE.get() else {
        return;
    };
    if !due(
        &mut s.last.lock().unwrap_or_else(|e| e.into_inner()),
        session,
        Instant::now(),
    ) {
        tracing::info!(session = ?session, "toast skipped: within 10 s");
        return;
    }
    let shown = s
        .app
        .notification()
        .builder()
        .title(format!("{project} needs you"))
        .body(body(tool))
        .extra("session", session)
        .show();
    tracing::info!(session = ?session, ok = shown.is_ok(), "toast shown");
}

/// Whether `session` may get a toast at `now`; if so, records `now`.
/// §10.2 body. The question tool arrives as a `PermissionRequest` (ADR 0010).
fn body(tool: Option<&str>) -> String {
    match tool {
        Some("AskUserQuestion") => "Claude Code has a question for you.".into(),
        Some(t) => format!("Claude Code is asking for permission to use {t}."),
        None => "Claude Code is asking for permission.".into(),
    }
}

fn due(last: &mut HashMap<String, Instant>, session: &str, now: Instant) -> bool {
    if last
        .get(session)
        .is_some_and(|&t| now.duration_since(t) < MIN_GAP)
    {
        return false;
    }
    last.insert(session.to_owned(), now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_one_toast_per_10_s_per_session() {
        let mut last = HashMap::new();
        let t = Instant::now();
        let s = Duration::from_secs;
        assert!(due(&mut last, "a", t));
        assert!(!due(&mut last, "a", t + s(9)));
        assert!(due(&mut last, "b", t + s(9)));
        assert!(due(&mut last, "a", t + s(10)));
        assert!(!due(&mut last, "a", t + s(19)));
    }

    #[test]
    fn question_tool_gets_the_question_text() {
        assert_eq!(
            body(Some("AskUserQuestion")),
            "Claude Code has a question for you."
        );
        assert_eq!(
            body(Some("Write")),
            "Claude Code is asking for permission to use Write."
        );
        assert_eq!(body(None), "Claude Code is asking for permission.");
    }
}
