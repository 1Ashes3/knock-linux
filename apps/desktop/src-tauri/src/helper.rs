//! `knock-desktop hook <agent> [<event>]` (ADR 0039): the command that Codex,
//! Copilot, Antigravity and other command-hook agents run for each event. It reads
//! the event from standard input and posts it to the running Knock, then exits.
//! `<event>` is for agents whose payloads don't name their event (agy): it is added
//! to the payload as `hook_event_name`.
//!
//! It must never get in an agent's way, so it:
//! - always exits 0, whether Knock runs or not, whether the post worked or not, and
//!   prints nothing (some agents read stdout as a decision), except `{}` for agents
//!   that insist on a JSON answer ([`reply`]);
//! - gives up quickly: 1 s for the event to arrive on stdin, 1 s for Knock;
//! - never starts the app, a window or the log: this runs before any of it.
//!
//! The token never appears in a command line: the helper reads it from
//! `server.json` itself. The server tells the helper from the agent by
//! `X-Knock-Via: hook` and attributes the session to the agent above it
//! ([`crate::jump::agent_of`]), since the helper is gone a moment later.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// How long the event may take to arrive, and Knock to answer.
const WAIT: Duration = Duration::from_secs(1);
/// Same limit as the server's body limit (§8.1).
const MAX_BODY: u64 = 16 * 1024 * 1024;

/// If this process was started as `knock-desktop hook <agent>`, runs the
/// helper and exits; otherwise returns and the app starts.
pub fn run_if_asked() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (agent, event) = match args.as_slice() {
        [command, agent] if command == "hook" => (agent, None),
        [command, agent, event] if command == "hook" => (agent, Some(event.as_str())),
        _ => return,
    };
    if knock_core::AGENTS.contains(&agent.as_str())
        && let Some(body) = read_stdin()
    {
        let body = event.map_or(body.clone(), |e| with_event(&body, e));
        let _ = send(agent, &body);
    }
    print!("{}", reply(agent));
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

/// What the helper prints for `agent`: agy parses a hook's output as JSON and
/// warns about anything else, so it gets `{}` (no decision); the others get
/// nothing, which they all take as "no decision".
fn reply(agent: &str) -> &'static str {
    if agent == "agy" { "{}\n" } else { "" }
}

/// `body` (a JSON object) with `hook_event_name` set to `event`; the body
/// unchanged if it isn't an object, so a surprise never stops the post.
fn with_event(body: &[u8], event: &str) -> Vec<u8> {
    let mut v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return body.to_vec(),
    };
    match v.as_object_mut() {
        Some(o) => {
            o.insert("hook_event_name".into(), event.into());
            serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec())
        }
        None => body.to_vec(),
    }
}

/// Standard input to its end, or `None` if it doesn't end within [`WAIT`]
/// (an agent that keeps it open) or is empty.
fn read_stdin() -> Option<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut body = Vec::new();
        let _ = std::io::stdin().take(MAX_BODY).read_to_end(&mut body);
        let _ = tx.send(body);
    });
    rx.recv_timeout(WAIT).ok().filter(|b| !b.is_empty())
}

/// One POST to the running Knock; any failure (not running, refused) is
/// just an error nobody looks at.
fn send(agent: &str, body: &[u8]) -> std::io::Result<()> {
    let dir = crate::settings::data_dir()?;
    let cfg = crate::server::parse_config(&std::fs::read(dir.join("server.json"))?)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.port));
    let mut s = TcpStream::connect_timeout(&addr, WAIT)?;
    s.set_read_timeout(Some(WAIT))?;
    s.set_write_timeout(Some(WAIT))?;
    write!(
        s,
        "POST /v1/hooks/{agent} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         Content-Type: application/json\r\nX-Knock-Token: {}\r\nX-Knock-Via: hook\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        cfg.token,
        body.len()
    )?;
    s.write_all(body)?;
    // Until Knock has answered: its process lookup happens before the reply,
    // and needs this process to still be here.
    let mut reply = Vec::new();
    s.read_to_end(&mut reply).map(|_| ())
}

/// The command a hook runs for `agent` (ADR 0039): this program, `hook`, the
/// agent's id and, for agents whose payloads don't carry it, the event, quoted so
/// a path with spaces works.
// Used by the agents' hook setup, which comes next.
#[allow(dead_code)]
pub fn command(exe: &std::path::Path, agent: &str, event: Option<&str>) -> String {
    let event = event.map(|e| format!(" {e}")).unwrap_or_default();
    format!("\"{}\" hook {agent}{event}", exe.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_command_quotes_the_program() {
        let exe = std::path::Path::new("/home/a b/Apps/Knock.AppImage");
        assert_eq!(
            command(exe, "codex", None),
            r#""/home/a b/Apps/Knock.AppImage" hook codex"#
        );
        assert_eq!(
            command(exe, "agy", Some("PreToolUse")),
            r#""/home/a b/Apps/Knock.AppImage" hook agy PreToolUse"#
        );
    }

    #[test]
    fn the_event_is_added_to_the_payload_and_nothing_else_changes() {
        let body = br#"{"conversationId":"c1","toolCall":{"name":"read_file"}}"#;
        let v: serde_json::Value = serde_json::from_slice(&with_event(body, "PreToolUse")).unwrap();
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(
            (v["conversationId"].as_str(), v["toolCall"]["name"].as_str()),
            (Some("c1"), Some("read_file"))
        );
        // Not an object, or not JSON: passed on untouched.
        assert_eq!(with_event(b"[1]", "Stop"), b"[1]");
        assert_eq!(with_event(b"not json", "Stop"), b"not json");
    }

    #[test]
    fn only_agy_is_answered_with_json() {
        assert_eq!(reply("agy"), "{}\n");
        for agent in ["codex", "copilot", "claude-code"] {
            assert_eq!(reply(agent), "", "{agent}");
        }
    }
}
