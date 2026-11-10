//! Hook setup for the other agents (ADR 0039): Codex CLI, GitHub Copilot CLI and
//! Antigravity CLI (`agy`). The same promise as for Claude Code (§8.2): setup shows
//! exactly what it will change, backs the file up, writes only after the user
//! confirms, and removal takes out only Knock's entries.
//!
//! These agents run a *command* for each hook, so Knock's entry is
//! `"<knock-desktop>" hook <agent> [<event>]` (see [`crate::helper`]): no token
//! in the file, and nothing to repair when the server's port changes. Each agent
//! has its own layout:
//!
//! | agent | file | layout |
//! |---|---|---|
//! | Codex | `~/.codex/hooks.json` | Claude's: `hooks` → event → matcher groups → handlers |
//! | Copilot | `~/.copilot/hooks/knock.json` | a file of Knock's own: `version`, `hooks` → event → handlers |
//! | agy | `~/.gemini/config/hooks.json` | one named suite, `"knock"`, beside the user's |
//!
//! Knock's handlers are recognised by their command ([`is_knock_command`]), so a
//! Knock that moved (an update, a moved AppImage) is found and replaced.

use crate::hooks::{self, J, Problem};
use knock_core::HookSpec;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The agents set up here, in the order Settings lists them.
pub const IDS: [&str; 3] = ["codex", "copilot", "agy"];

/// A hook may take this long before the agent gives up on it, in seconds. The
/// helper needs about 40 ms and never waits more than 2 s.
const TIMEOUT: i64 = 2;

/// What the user is told to do once Knock is in the agent's file.
pub fn after_note(agent: &str) -> &'static str {
    match agent {
        "codex" => {
            "Open Codex and run /hooks once to trust Knock's hooks; Codex skips new hooks until you do."
        }
        "copilot" => "Restart GitHub Copilot CLI: it reads its hooks when it starts.",
        "agy" => "Start a new Antigravity conversation: it reads its hooks when it starts.",
        _ => "",
    }
}

pub fn display_name(agent: &str) -> &'static str {
    match agent {
        "codex" => "Codex",
        "copilot" => "GitHub Copilot",
        "agy" => "Antigravity",
        "claude-code" => "Claude Code",
        _ => "this agent",
    }
}

/// The command a hook runs for `agent`'s `event` (the helper adds the event to payloads
/// that don't carry it): Knock's program, quoted, then `hook <agent> [<event>]`.
fn command(exe: &Path, agent: &str, event: Option<&str>) -> String {
    crate::helper::command(exe, agent, event)
}

/// A command that starts Knock's hook helper for `agent`: Knock's own program (any
/// folder, quoted or not, any case, so an older copy counts too: `knock`,
/// `knock-desktop` or `Knock*.AppImage`, with `.exe` on Windows), then
/// `hook <agent>`, then at most the event. A user's own `knock-notify` is not Knock's.
pub fn is_knock_command(cmd: &str, agent: &str) -> bool {
    let cmd = cmd.trim_start();
    let (program, rest) = match cmd.strip_prefix('"') {
        Some(q) => match q.split_once('"') {
            Some((p, r)) => (p, r),
            None => return false,
        },
        None => cmd.split_once(' ').unwrap_or((cmd, "")),
    };
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    let ours = stem == "knock"
        || stem == "knock-desktop"
        || (name.starts_with("knock") && name.ends_with(".appimage"));
    if !ours {
        return false;
    }
    let mut words = rest.split_whitespace();
    words.next() == Some("hook")
        && words.next() == Some(agent)
        && words
            .next()
            .is_none_or(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
        && words.next().is_none()
}

fn handler(exe: &Path, agent: &str, event: Option<&str>) -> J {
    let cmd = command(exe, agent, event);
    match agent {
        // Copilot names the shell: `bash` on Linux and macOS, `powershell` on Windows.
        "copilot" => {
            let key = if cfg!(windows) { "powershell" } else { "bash" };
            let cmd = if cfg!(windows) {
                format!("& {cmd}")
            } else {
                cmd
            };
            J::Obj(vec![
                ("type".into(), J::Str("command".into())),
                (key.into(), J::Str(cmd)),
                ("timeoutSec".into(), J::Num(TIMEOUT.into())),
            ])
        }
        _ => J::Obj(vec![
            ("type".into(), J::Str("command".into())),
            ("command".into(), J::Str(cmd)),
            ("timeout".into(), J::Num(TIMEOUT.into())),
        ]),
    }
}

/// The command text of a handler, whichever key the agent uses.
fn handler_command(h: &J) -> Option<&str> {
    ["command", "bash", "powershell"]
        .iter()
        .find_map(|k| h.get(k).and_then(J::str))
}

fn is_knock(h: &J, agent: &str) -> bool {
    handler_command(h).is_some_and(|c| is_knock_command(c.trim_start_matches("& "), agent))
}

fn spec(agent: &str) -> Option<HookSpec> {
    knock_core::adapter(agent, Vec::new()).map(|a| a.hooks())
}

/// Whether the file holds the current hooks, Knock's hooks from another install,
/// or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Installed,
    /// Knock's entries are there but point elsewhere or are incomplete (Knock moved,
    /// or an old version wrote them): connecting again fixes them.
    Stale,
    Missing,
}

/// What setup will write: the whole file before and after (`after` is `None`
/// when the file is deleted).
pub struct Plan {
    pub before: String,
    pub after: Option<String>,
}

fn the_file(raw: Option<&[u8]>) -> Result<J, Problem> {
    hooks::parse(raw)
}

fn render(v: &J, raw: Option<&[u8]>) -> String {
    hooks::render(v, hooks::text(raw).as_deref())
}

// -------------------------------------------------------------------- Codex

fn codex_install(v: &mut J, exe: &Path) -> Result<(), Problem> {
    let s = spec("codex").ok_or(Problem::Shape)?;
    hooks::install_with(v, s.events, &|_| handler(exe, "codex", None), &|h| {
        is_knock(h, "codex")
    })
}

fn codex_state(v: &J, exe: &Path) -> State {
    let Some(s) = spec("codex") else {
        return State::Missing;
    };
    let ours: Vec<&J> = hooks::handlers(v)
        .filter(|h| is_knock(h, "codex"))
        .collect();
    if ours.is_empty() {
        return State::Missing;
    }
    let want = serde_json::to_value(handler(exe, "codex", None)).ok();
    let right = |event: &str, matcher: Option<&str>| {
        let groups = v.get("hooks").and_then(|h| h.get(event)).and_then(J::arr);
        groups.into_iter().flatten().any(|g| {
            g.get("matcher").and_then(J::str) == matcher
                && (g.get("hooks").and_then(J::arr))
                    .is_some_and(|hs| hs.iter().any(|h| serde_json::to_value(h).ok() == want))
        })
    };
    let all = s.events.iter().all(|(e, m)| right(e, *m));
    if all && ours.len() == s.events.len() {
        State::Installed
    } else {
        State::Stale
    }
}

fn codex_remove(v: &mut J) -> Result<usize, Problem> {
    let s = spec("codex").ok_or(Problem::Shape)?;
    hooks::strip_with(v, s.events, &|h| is_knock(h, "codex"), false)
}

// ------------------------------------------------------------------- Copilot

/// The whole file, Knock's own: every event's handler array holds one handler.
fn copilot_file(exe: &Path) -> Result<J, Problem> {
    let s = spec("copilot").ok_or(Problem::Shape)?;
    let events = (s.events.iter())
        .map(|(e, _)| {
            (
                e.to_string(),
                J::Arr(vec![handler(exe, "copilot", Some(e))]),
            )
        })
        .collect();
    Ok(J::Obj(vec![
        ("version".into(), J::Num(1.into())),
        ("hooks".into(), J::Obj(events)),
    ]))
}

/// Every handler in a Copilot file.
fn copilot_handlers(v: &J) -> Vec<&J> {
    match v.get("hooks") {
        Some(J::Obj(m)) => m.iter().filter_map(|(_, hs)| hs.arr()).flatten().collect(),
        _ => Vec::new(),
    }
}

/// The file is Knock's own: nothing in it is anyone else's.
fn copilot_is_ours(v: &J) -> bool {
    copilot_handlers(v).iter().all(|h| is_knock(h, "copilot"))
}

fn copilot_state(v: &J, exe: &Path) -> State {
    let want = copilot_file(exe)
        .ok()
        .and_then(|f| serde_json::to_value(f).ok());
    let have = serde_json::to_value(v).ok();
    if copilot_handlers(v).is_empty() {
        State::Missing
    } else if have == want {
        State::Installed
    } else {
        State::Stale
    }
}

// ----------------------------------------------------------------------- agy

const AGY_SUITE: &str = "knock";

/// agy's suite of Knock hooks: flat handler lists for the lifecycle events, matcher
/// groups for the two tool events (the layout agy documents and ignores no other).
fn agy_suite(exe: &Path) -> Result<J, Problem> {
    let s = spec("agy").ok_or(Problem::Shape)?;
    let mut suite = vec![("enabled".to_string(), J::Bool(true))];
    for (event, matcher) in s.events {
        let h = handler(exe, "agy", Some(event));
        let entry = match matcher {
            Some(m) => J::Arr(vec![J::Obj(vec![
                ("matcher".into(), J::Str(m.to_string())),
                ("hooks".into(), J::Arr(vec![h])),
            ])]),
            None => J::Arr(vec![h]),
        };
        suite.push((event.to_string(), entry));
    }
    Ok(J::Obj(suite))
}

/// Every handler in the suite, in either layout.
fn agy_handlers(suite: &J) -> Vec<&J> {
    let J::Obj(m) = suite else { return Vec::new() };
    (m.iter())
        .filter_map(|(_, v)| v.arr())
        .flatten()
        .flat_map(|g| match g.get("hooks").and_then(J::arr) {
            Some(hs) => hs.iter().collect(),
            None => vec![g],
        })
        .collect()
}

fn agy_is_ours(suite: &J) -> bool {
    agy_handlers(suite).iter().all(|h| is_knock(h, "agy"))
}

fn agy_install(v: &mut J, exe: &Path) -> Result<(), Problem> {
    let suite = agy_suite(exe)?;
    if let Some(old) = v.get(AGY_SUITE)
        && !agy_is_ours(old)
    {
        // Someone else's suite with our name: not ours to replace.
        return Err(Problem::Shape);
    }
    let J::Obj(m) = v else {
        return Err(Problem::Shape);
    };
    match m.iter_mut().rev().find(|(k, _)| k == AGY_SUITE) {
        Some((_, slot)) => *slot = suite,
        None => m.push((AGY_SUITE.into(), suite)),
    }
    Ok(())
}

fn agy_state(v: &J, exe: &Path) -> State {
    match v.get(AGY_SUITE) {
        None => State::Missing,
        Some(have) if agy_handlers(have).is_empty() => State::Missing,
        Some(have) => {
            let want = agy_suite(exe)
                .ok()
                .and_then(|s| serde_json::to_value(s).ok());
            if serde_json::to_value(have).ok() == want {
                State::Installed
            } else {
                State::Stale
            }
        }
    }
}

fn agy_remove(v: &mut J) -> Result<usize, Problem> {
    let J::Obj(m) = v else {
        return Err(Problem::Shape);
    };
    let Some(i) = m.iter().rposition(|(k, _)| k == AGY_SUITE) else {
        return Ok(0);
    };
    if !agy_is_ours(&m[i].1) {
        return Err(Problem::Shape);
    }
    let n = agy_handlers(&m[i].1).len();
    m.remove(i);
    Ok(n)
}

// ------------------------------------------------------------------- planning

/// What the file will be after connecting `agent`.
pub fn plan_install(agent: &str, raw: Option<&[u8]>, exe: &Path) -> Result<Plan, Problem> {
    let mut v = the_file(raw)?;
    match agent {
        "codex" => codex_install(&mut v, exe)?,
        "copilot" => {
            if !copilot_is_ours(&v) {
                return Err(Problem::Shape);
            }
            v = copilot_file(exe)?;
        }
        "agy" => agy_install(&mut v, exe)?,
        _ => return Err(Problem::Shape),
    }
    Ok(Plan {
        before: hooks::text(raw).unwrap_or_default(),
        after: Some(render(&v, raw)),
    })
}

/// What the file will be after removing Knock from `agent`'s.
pub fn plan_remove(agent: &str, raw: Option<&[u8]>) -> Result<Plan, Problem> {
    let mut v = the_file(raw)?;
    let before = hooks::text(raw).unwrap_or_default();
    let after = match agent {
        "codex" => {
            if codex_remove(&mut v)? == 0 {
                return Err(Problem::NothingToRemove);
            }
            Some(render(&v, raw))
        }
        "copilot" => {
            if copilot_handlers(&v).is_empty() {
                return Err(Problem::NothingToRemove);
            }
            if !copilot_is_ours(&v) {
                return Err(Problem::Shape);
            }
            // The file was Knock's alone: it goes.
            None
        }
        "agy" => {
            if agy_remove(&mut v)? == 0 {
                return Err(Problem::NothingToRemove);
            }
            Some(render(&v, raw))
        }
        _ => return Err(Problem::Shape),
    };
    Ok(Plan { before, after })
}

/// What `agent`'s file holds of Knock's.
pub fn state(agent: &str, raw: Option<&[u8]>, exe: &Path) -> Result<State, Problem> {
    let v = the_file(raw)?;
    Ok(match agent {
        "codex" => codex_state(&v, exe),
        "copilot" => copilot_state(&v, exe),
        "agy" => agy_state(&v, exe),
        _ => State::Missing,
    })
}

// ---------------------------------------------------------------------- paths

/// Where `agent`'s hooks file is, for `env`. A debug build never touches the real
/// ones: it works only under `KNOCK_AGENTS_HOME` (like `KNOCK_CLAUDE_SETTINGS` for
/// Claude Code), and there is no file without it.
pub fn config_path(
    agent: &str,
    env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = if cfg!(debug_assertions) {
        var("KNOCK_AGENTS_HOME")?
    } else {
        knock_core::paths::home_dir(env)?
    };
    // `CODEX_HOME` and `COPILOT_HOME` move the agents' folders; not in a debug build.
    let moved = |k: &str| if cfg!(debug_assertions) { None } else { var(k) };
    Some(match agent {
        "codex" => moved("CODEX_HOME")
            .unwrap_or_else(|| home.join(".codex"))
            .join("hooks.json"),
        "copilot" => moved("COPILOT_HOME")
            .unwrap_or_else(|| home.join(".copilot"))
            .join("hooks")
            .join("knock.json"),
        "agy" => home.join(".gemini").join("config").join("hooks.json"),
        _ => return None,
    })
}

/// The program names `agent` is run as.
fn programs(agent: &str) -> &'static [&'static str] {
    match agent {
        "codex" => &["codex"],
        "copilot" => &["copilot"],
        "agy" => &["agy", "antigravity"],
        _ => &[],
    }
}

/// `agent` is on this computer: its program is on `PATH`, or its folder exists.
pub fn found(agent: &str, config: Option<&Path>, path_var: Option<&std::ffi::OsStr>) -> bool {
    // The agent's own folder is the file's folder, or the one above it.
    let folder = config.and_then(|c| {
        // Codex's file is in its folder; Copilot's and agy's are one level down.
        let up = if agent == "codex" { 1 } else { 2 };
        c.ancestors().nth(up)
    });
    if folder.is_some_and(Path::is_dir) {
        return true;
    }
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd"]
    } else {
        &[""]
    };
    path_var.is_some_and(|p| {
        std::env::split_paths(p).any(|dir| {
            programs(agent).iter().any(|name| {
                exts.iter()
                    .any(|e| dir.join(format!("{name}{e}")).is_file())
            })
        })
    })
}

// ---------------------------------------------------------------------- setup

/// The preview the user is looking at.
struct Previewed {
    raw: Option<Vec<u8>>,
    after: Option<String>,
}

/// Setup for one agent. One operation at a time.
pub struct AgentSetup {
    pub agent: &'static str,
    exe: PathBuf,
    /// `None` in a debug build without `KNOCK_AGENTS_HOME`.
    pub path: Option<PathBuf>,
    backups: PathBuf,
    path_var: Option<std::ffi::OsString>,
    previewed: Mutex<Option<Previewed>>,
}

pub struct Preview {
    pub path: String,
    pub before: String,
    pub after: String,
    /// The file will be deleted (it was Knock's alone).
    pub deletes: bool,
}

/// One agent's line in Settings.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: &'static str,
    pub name: &'static str,
    /// Its program or folder is on this computer.
    pub found: bool,
    /// `None` when the file can't be read or changed; `problem` says why.
    pub state: Option<State>,
    pub problem: Option<String>,
    /// Said after connecting: what the user does next.
    pub note: &'static str,
}

/// A setup problem in words, for `agent_name`'s hooks file.
pub fn message(p: Problem, agent_name: &str) -> String {
    match p {
        Problem::NoPath => "This is a development build of Knock, so it never changes your real agent settings. Set KNOCK_AGENTS_HOME to a test folder to try this.".into(),
        Problem::Unreadable => format!("Knock couldn't open {agent_name}'s hooks file, so it didn't change anything. Please try again."),
        Problem::NotJson => format!("{agent_name}'s hooks file isn't valid JSON, so Knock didn't change it. Please fix the file, or ask for help, then try again."),
        Problem::Shape => format!("{agent_name}'s hooks file is laid out in a way Knock doesn't recognise (or part of it belongs to something else), so Knock didn't change it."),
        Problem::NoPreview => "Please review the change first.".into(),
        Problem::Stale => format!("{agent_name}'s hooks file changed since the preview; please review again."),
        Problem::Backup => format!("Knock couldn't save a copy of {agent_name}'s hooks file first, so it didn't change it. Please try again."),
        Problem::Write => format!("Knock couldn't save {agent_name}'s hooks file. Nothing was changed. Please try again."),
        Problem::NothingToRemove => format!("Knock isn't in {agent_name}'s hooks file, so there was nothing to remove."),
        Problem::Linked => format!("Knock can't safely edit {agent_name}'s hooks file because it's linked from another place."),
        other => other.message().to_string(),
    }
}

impl AgentSetup {
    pub fn new(
        agent: &'static str,
        exe: PathBuf,
        path: Option<PathBuf>,
        backups: PathBuf,
        path_var: Option<std::ffi::OsString>,
    ) -> Self {
        Self {
            agent,
            exe,
            path,
            backups: backups.join(agent),
            path_var,
            previewed: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Previewed>> {
        self.previewed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn file(&self) -> Result<PathBuf, Problem> {
        hooks::target(self.path.as_deref().ok_or(Problem::NoPath)?, &[])
    }

    pub fn status(&self) -> Status {
        let _last = self.lock();
        let found = found(self.agent, self.path.as_deref(), self.path_var.as_deref());
        let result = self
            .file()
            .and_then(|p| hooks::read(&p))
            .and_then(|raw| state(self.agent, raw.as_deref(), &self.exe));
        let (state, problem) = match result {
            Ok(s) => (Some(s), None),
            Err(p) => (None, Some(message(p, display_name(self.agent)))),
        };
        Status {
            id: self.agent,
            name: display_name(self.agent),
            found,
            state,
            problem,
            note: after_note(self.agent),
        }
    }

    /// The file before and after; remembered so `apply` can tell whether it changed since.
    pub fn preview(&self, remove: bool) -> Result<Preview, Problem> {
        let mut last = self.lock();
        *last = None;
        let path = self.file()?;
        let raw = hooks::read(&path)?;
        let plan = if remove {
            plan_remove(self.agent, raw.as_deref())?
        } else {
            plan_install(self.agent, raw.as_deref(), &self.exe)?
        };
        let preview = Preview {
            path: hooks::shown(&path),
            before: plan.before,
            after: plan.after.clone().unwrap_or_default(),
            deletes: plan.after.is_none(),
        };
        *last = Some(Previewed {
            raw,
            after: plan.after,
        });
        Ok(preview)
    }

    /// Writes what the last preview showed, re-read and re-computed now; if the
    /// file or the result differs, or the file changes before the write:
    /// [`Problem::Stale`]. Returns the backup's path, or `""` when there was no
    /// file to back up.
    pub fn apply(&self, remove: bool, now_ms: u64) -> Result<String, Problem> {
        let mut last = self.lock();
        let path = self.file()?;
        let shown = last.take().ok_or(Problem::NoPreview)?;
        let raw = hooks::read(&path)?;
        if raw != shown.raw {
            return Err(Problem::Stale);
        }
        let plan = if remove {
            plan_remove(self.agent, raw.as_deref())?
        } else {
            plan_install(self.agent, raw.as_deref(), &self.exe)?
        };
        if plan.after != shown.after {
            return Err(Problem::Stale);
        }
        let raw = raw.as_deref();
        let still = || hooks::unchanged(&path, raw);
        let saved = match &plan.after {
            Some(after) => hooks::commit(&path, &self.backups, raw, after, now_ms, still)?,
            None => delete(&path, &self.backups, raw, now_ms, still)?,
        };
        tracing::info!(agent = self.agent, remove, "agent hooks changed");
        Ok(saved)
    }
}

/// Backs the file up, then deletes it if it is still as it was.
fn delete(
    path: &Path,
    backups: &Path,
    original: Option<&[u8]>,
    now_ms: u64,
    still: impl FnOnce() -> bool,
) -> Result<String, Problem> {
    let saved = match original {
        Some(o) => hooks::backup(backups, o, now_ms)
            .map_err(|_| Problem::Backup)?
            .display()
            .to_string(),
        None => String::new(),
    };
    if !still() {
        return Err(Problem::Stale);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(saved),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(saved),
        Err(_) => Err(Problem::Write),
    }
}

/// Every agent's setup.
pub struct Agents {
    pub setups: Vec<AgentSetup>,
}

impl Agents {
    pub fn new(exe: PathBuf, backups: PathBuf) -> Self {
        let env = |k: &str| std::env::var_os(k);
        let path_var = std::env::var_os("PATH");
        let setups = IDS
            .iter()
            .map(|id| {
                AgentSetup::new(
                    id,
                    exe.clone(),
                    config_path(id, &env),
                    backups.clone(),
                    path_var.clone(),
                )
            })
            .collect();
        Self { setups }
    }

    pub fn get(&self, id: &str) -> Option<&AgentSetup> {
        self.setups.iter().find(|s| s.agent == id)
    }
}

/// `{ ok: true, ... }` shapes for the UI.
pub fn preview_json(p: &Preview) -> Value {
    json!({ "ok": true, "path": p.path, "before": p.before, "after": p.after, "deletes": p.deletes })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/opt/Knock Apps/knock-desktop";

    fn exe() -> PathBuf {
        PathBuf::from(EXE)
    }

    fn after(agent: &str, raw: Option<&str>) -> String {
        plan_install(agent, raw.map(str::as_bytes), &exe())
            .unwrap()
            .after
            .unwrap()
    }

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn knocks_commands_are_recognised_wherever_knock_lives() {
        for ok in [
            r#""/opt/Knock Apps/knock-desktop" hook codex"#,
            r#""C:\Users\A\AppData\Local\Knock\knock-desktop.exe" hook codex"#,
            r#""/home/a/Apps/Knock-0.3.AppImage" hook agy PreToolUse"#,
            "/usr/bin/knock-desktop hook codex",
            "knock-desktop hook codex",
            r#"  "/old/KNOCK" hook codex "#,
        ] {
            let agent = if ok.contains(" agy") { "agy" } else { "codex" };
            assert!(is_knock_command(ok, agent), "{ok}");
        }
        for no in [
            r#""/usr/bin/notify-send" hook codex"#,
            r#""/opt/knock-desktop" hook copilot"#,
            r#""/opt/knock-desktop" hook codex && rm -rf ~"#,
            r#""/opt/knock-desktop" hook codex PreToolUse extra"#,
            r#""/opt/knock-desktop" hook codex ev;ent"#,
            r#""/opt/knock-desktop" run codex"#,
            r#"python3 "/opt/knock-desktop" hook codex"#,
            r#""/home/a/bin/knock-notify" hook codex"#,
            r#""/opt/knock-desktop.sh" hook codex"#,
            r#""/opt/knock-desktop hook codex"#,
            "",
        ] {
            assert!(!is_knock_command(no, "codex"), "{no}");
        }
    }

    const CODEX_USER: &str = r#"{
  "description": "mine",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "./check.sh", "timeout": 30 } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "./done.sh" } ] }
    ]
  }
}
"#;

    #[test]
    fn codex_gets_one_handler_per_event_and_the_users_hooks_stay() {
        let out = after("codex", Some(CODEX_USER));
        let o = v(&out);
        assert_eq!(o["description"], "mine");
        // The user's own handlers are untouched.
        assert_eq!(
            o["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "./check.sh"
        );
        let stop = o["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop[0]["hooks"][0]["command"], "./done.sh");
        // Knock's: one per registered event, matcher on tool events, command quoted.
        let want = format!("\"{EXE}\" hook codex");
        for (event, matcher) in spec("codex").unwrap().events {
            let groups = o["hooks"][event].as_array().unwrap();
            let ours: Vec<_> = (groups.iter())
                .filter(|g| g["hooks"][0]["command"] == want.as_str())
                .collect();
            assert_eq!(ours.len(), 1, "{event}");
            assert_eq!(ours[0]["matcher"].as_str(), *matcher, "{event}");
            assert_eq!(ours[0]["hooks"][0]["timeout"], TIMEOUT);
        }
        // Never a PreToolUse of Knock's own: a failing hook there blocks the tool.
        assert_eq!(o["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        // Indentation is the file's, and installing again changes nothing.
        assert!(out.contains("\n  \"hooks\""));
        assert_eq!(after("codex", Some(&out)), out);
        // Removal gives back exactly the user's file.
        let removed = plan_remove("codex", Some(out.as_bytes()))
            .unwrap()
            .after
            .unwrap();
        assert_eq!(v(&removed), v(CODEX_USER));
    }

    #[test]
    fn codex_state_tells_installed_stale_and_missing() {
        let st =
            |raw: Option<&str>, exe: &Path| state("codex", raw.map(str::as_bytes), exe).unwrap();
        assert_eq!(st(None, &exe()), State::Missing);
        assert_eq!(st(Some(CODEX_USER), &exe()), State::Missing);
        let ours = after("codex", None);
        assert_eq!(st(Some(&ours), &exe()), State::Installed);
        // Knock moved: its old entries are stale, and connecting replaces them all.
        let moved = Path::new("/home/a/Apps/Knock.AppImage");
        assert_eq!(st(Some(&ours), moved), State::Stale);
        let fixed = plan_install("codex", Some(ours.as_bytes()), moved)
            .unwrap()
            .after
            .unwrap();
        assert_eq!(
            state("codex", Some(fixed.as_bytes()), moved).unwrap(),
            State::Installed
        );
        assert!(!fixed.contains("Knock Apps"), "no old entry left");
        // Half-removed: stale.
        let mut half = v(&ours);
        half["hooks"].as_object_mut().unwrap().remove("Stop");
        assert_eq!(st(Some(&half.to_string()), &exe()), State::Stale);
    }

    #[test]
    fn copilot_has_a_file_of_its_own_with_camel_case_events() {
        let out = after("copilot", None);
        let o = v(&out);
        assert_eq!(o["version"], 1);
        let hooks = o["hooks"].as_object().unwrap();
        let names: Vec<_> = hooks.keys().map(String::as_str).collect();
        let mut want = vec![
            "userPromptSubmitted",
            "postToolUse",
            "notification",
            "agentStop",
            "sessionEnd",
        ];
        want.sort();
        let mut got = names.clone();
        got.sort();
        assert_eq!(got, want);
        let key = if cfg!(windows) { "powershell" } else { "bash" };
        let h = &hooks["agentStop"][0];
        assert_eq!(h["type"], "command");
        assert!(
            h[key].as_str().unwrap().ends_with("hook copilot agentStop"),
            "{h}"
        );
        assert_eq!(h["timeoutSec"], TIMEOUT);
        // Never the two events that could block a tool or knock when nothing asks.
        assert!(!out.contains("preToolUse") && !out.contains("permissionRequest"));
        assert_eq!(
            state("copilot", Some(out.as_bytes()), &exe()).unwrap(),
            State::Installed
        );
        assert_eq!(state("copilot", None, &exe()).unwrap(), State::Missing);
        // Removing it deletes the file; it was Knock's alone.
        let plan = plan_remove("copilot", Some(out.as_bytes())).unwrap();
        assert!(plan.after.is_none());
        // Someone else's handler in it: not ours to overwrite or delete.
        let mut mine = o.clone();
        mine["hooks"]["agentStop"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"command","bash":"./mine.sh"}));
        assert!(plan_install("copilot", Some(mine.to_string().as_bytes()), &exe()).is_err());
        assert!(plan_remove("copilot", Some(mine.to_string().as_bytes())).is_err());
        let moved = Path::new("/elsewhere/knock-desktop");
        assert_eq!(
            state("copilot", Some(out.as_bytes()), moved).unwrap(),
            State::Stale
        );
    }

    const AGY_USER: &str = r#"{
	"mine": {
		"enabled": true,
		"Stop": [ { "type": "command", "command": "./notify.sh", "timeout": 5 } ]
	}
}
"#;

    #[test]
    fn agy_gets_one_named_suite_beside_the_users() {
        let out = after("agy", Some(AGY_USER));
        let o = v(&out);
        assert_eq!(o["mine"]["Stop"][0]["command"], "./notify.sh");
        let k = &o["knock"];
        assert_eq!(k["enabled"], true);
        // Lifecycle events are flat handler lists; the tool events are matcher groups.
        let flat = &k["Stop"][0];
        assert_eq!(flat["command"], format!("\"{EXE}\" hook agy Stop"));
        assert_eq!(flat["timeout"], TIMEOUT);
        let grouped = &k["PostToolUse"][0];
        assert_eq!(grouped["matcher"], "*");
        assert_eq!(
            grouped["hooks"][0]["command"],
            format!("\"{EXE}\" hook agy PostToolUse")
        );
        // Never PreToolUse: agy denies every tool call unless it answers "allow".
        assert!(k.get("PreToolUse").is_none());
        for event in [
            "SessionStart",
            "PreInvocation",
            "PostToolUse",
            "PostInvocation",
        ] {
            assert!(k[event].is_array(), "{event}");
        }
        // The file's own (tab) indentation, idempotent, and removal restores the user's file.
        assert!(out.contains("\n\t\"mine\""));
        assert_eq!(after("agy", Some(&out)), out);
        let removed = plan_remove("agy", Some(out.as_bytes()))
            .unwrap()
            .after
            .unwrap();
        assert_eq!(v(&removed), v(AGY_USER));
        assert_eq!(
            state("agy", Some(out.as_bytes()), &exe()).unwrap(),
            State::Installed
        );
        assert_eq!(
            state("agy", Some(AGY_USER.as_bytes()), &exe()).unwrap(),
            State::Missing
        );
        assert_eq!(
            state("agy", Some(out.as_bytes()), Path::new("/new/knock-desktop")).unwrap(),
            State::Stale
        );
        // A new file holds just the suite.
        assert_eq!(
            v(&after("agy", None))
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["knock"]
        );
    }

    #[test]
    fn a_suite_called_knock_that_isnt_ours_is_left_alone() {
        let theirs =
            r#"{ "knock": { "Stop": [ { "type": "command", "command": "./theirs.sh" } ] } }"#;
        assert_eq!(
            plan_install("agy", Some(theirs.as_bytes()), &exe()).err(),
            Some(Problem::Shape)
        );
        assert_eq!(
            plan_remove("agy", Some(theirs.as_bytes())).err(),
            Some(Problem::Shape)
        );
        assert_eq!(
            plan_remove("agy", Some(AGY_USER.as_bytes())).err(),
            Some(Problem::NothingToRemove)
        );
    }

    #[test]
    fn broken_or_odd_files_are_refused_before_anything_is_planned() {
        for agent in IDS {
            assert_eq!(
                plan_install(agent, Some(b"{ nope"), &exe()).err(),
                Some(Problem::NotJson),
                "{agent}"
            );
            assert_eq!(
                plan_install(agent, Some(b"[]"), &exe()).err(),
                Some(Problem::Shape),
                "{agent}"
            );
            assert!(plan_remove(agent, None).is_err(), "{agent}");
        }
        // A Codex file whose `hooks` isn't an object.
        assert_eq!(
            plan_install("codex", Some(br#"{"hooks": 3}"#), &exe()).err(),
            Some(Problem::Shape)
        );
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("knock-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Preview, then apply: a backup of what was there, an atomic write, and a
    /// stale preview refused.
    #[test]
    fn connecting_backs_up_writes_and_refuses_a_stale_preview() {
        let d = temp("flow");
        let file = d.join(".gemini/config/hooks.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, AGY_USER).unwrap();
        let s = AgentSetup::new("agy", exe(), Some(file.clone()), d.join("backups"), None);
        assert_eq!(s.status().state, Some(State::Missing));
        assert_eq!(
            s.apply(false, 1).err(),
            Some(Problem::NoPreview),
            "no apply without a preview"
        );
        let p = s.preview(false).unwrap();
        assert!(p.after.contains("\"knock\"") && !p.deletes && p.before == AGY_USER);
        // The file changes after the preview: refused, nothing written.
        std::fs::write(&file, r#"{"other":{}}"#).unwrap();
        assert_eq!(s.apply(false, 2).err(), Some(Problem::Stale));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"{"other":{}}"#);
        // A fresh preview and apply.
        std::fs::write(&file, AGY_USER).unwrap();
        s.preview(false).unwrap();
        let backup = s.apply(false, 3).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup).unwrap(),
            AGY_USER,
            "the user's file, saved first"
        );
        assert_eq!(s.status().state, Some(State::Installed));
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("hook agy Stop")
        );
        // Remove: previewed, then back to the user's file.
        let p = s.preview(true).unwrap();
        assert!(!p.after.contains("knock"));
        s.apply(true, 4).unwrap();
        assert_eq!(v(&std::fs::read_to_string(&file).unwrap()), v(AGY_USER));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn copilots_file_is_created_and_deleted_with_a_backup() {
        let d = temp("copilot");
        let file = d.join(".copilot/hooks/knock.json");
        let s = AgentSetup::new(
            "copilot",
            exe(),
            Some(file.clone()),
            d.join("backups"),
            None,
        );
        s.preview(false).unwrap();
        assert_eq!(
            s.apply(false, 1).unwrap(),
            "",
            "no file, nothing to back up"
        );
        assert!(file.exists());
        assert_eq!(s.status().state, Some(State::Installed));
        let p = s.preview(true).unwrap();
        assert!(p.deletes && p.after.is_empty());
        let backup = s.apply(true, 2).unwrap();
        assert!(!file.exists());
        assert!(
            std::fs::read_to_string(backup)
                .unwrap()
                .contains("agentStop")
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn agents_are_found_by_their_folder_or_their_program() {
        let d = temp("found");
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let none = d.join("nothing/.codex/hooks.json");
        assert!(!found("codex", Some(&none), Some(bin.as_os_str())));
        std::fs::write(bin.join("codex"), b"").unwrap();
        assert!(
            found("codex", Some(&none), Some(bin.as_os_str())),
            "on PATH"
        );
        std::fs::create_dir_all(d.join(".gemini/config")).unwrap();
        let g = d.join(".gemini/config/hooks.json");
        assert!(found("agy", Some(&g), None), "its folder exists");
        std::fs::create_dir_all(d.join(".copilot")).unwrap();
        let c = d.join(".copilot/hooks/knock.json");
        assert!(found("copilot", Some(&c), None));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn paths_follow_each_agents_own_folder() {
        let env = |k: &str| match k {
            "KNOCK_AGENTS_HOME" => Some("/h".into()),
            _ => None,
        };
        if cfg!(debug_assertions) {
            assert_eq!(
                config_path("codex", &env),
                Some(PathBuf::from("/h/.codex/hooks.json"))
            );
            assert_eq!(
                config_path("copilot", &env),
                Some(PathBuf::from("/h/.copilot/hooks/knock.json"))
            );
            assert_eq!(
                config_path("agy", &env),
                Some(PathBuf::from("/h/.gemini/config/hooks.json"))
            );
            // A debug build never touches the real ones.
            assert_eq!(config_path("codex", &|_| None), None);
        }
        assert_eq!(config_path("gemini", &env), None);
    }
}
