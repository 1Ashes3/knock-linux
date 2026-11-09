//! Claude Code hook setup and removal (SPEC.md §8.2; ADR 0005, 0006, 0007,
//! 0010). The event list and the server path come from the adapter's
//! `hooks()` data, so they have one source.
//!
//! Knock's entries are the handlers whose `url` starts with
//! `http://127.0.0.1:` and contains the adapter's path (§8.2 ownership).
//! Everything else in the file is kept: other keys, other hooks, their order
//! and their values. The file is written back as pretty JSON in its own
//! indentation (tabs or N spaces; 2, the way Claude Code writes it, for a
//! new or one-line file), keeping its trailing newline and CRLF line ends.
//!
//! A symlinked file is read and written at its final target; a file with
//! more than one hard link is refused, since replacing it would split it.
//!
//! Nothing here logs the file, the token or a path (§11.5).

use knock_core::HookSpec;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Number, Value, json};
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

const OWNER: &str = "http://127.0.0.1:";
const TOKEN_HEADER: &str = "X-Knock-Token";
const BOM: &[u8] = b"\xEF\xBB\xBF";
/// Shown instead of the token in a preview.
pub const MASK: &str = "••••";

/// Why setup did not write. Every message is plain language and holds no
/// path, token or file content, so it is safe to show and to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Debug build without `KNOCK_CLAUDE_SETTINGS`.
    NoPath,
    /// The local server isn't running, so there is no port or token.
    NoServer,
    Unreadable,
    NotJson,
    /// Valid JSON, but not the shape Claude Code uses where Knock writes.
    Shape,
    NoPreview,
    Stale,
    Backup,
    Write,
    NothingToRemove,
    /// The file has another hard link.
    Linked,
    /// Debug build: the file is the user's real Claude Code settings file,
    /// maybe through a link (ADR 0024).
    RealFile,
}

impl Problem {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoPath => {
                "This is a development build of Knock, so it never changes your real Claude Code settings. Set KNOCK_CLAUDE_SETTINGS to a test file to try setup."
            }
            Self::NoServer => {
                "Knock isn't listening for Claude Code right now, so it can't connect them yet. Choose Repair first, then try again."
            }
            Self::Unreadable => {
                "Knock couldn't open your Claude Code settings file, so it didn't change anything. Please try again."
            }
            Self::NotJson => {
                "Your Claude Code settings file isn't valid JSON, so Knock didn't change it. Please fix the file, or ask for help, then try again."
            }
            Self::Shape => {
                "Your Claude Code settings file is laid out in a way Knock doesn't recognise, so Knock didn't change it."
            }
            Self::NoPreview => "Please review the change first.",
            Self::Stale => "Your settings changed since the preview; please review again.",
            Self::Backup => {
                "Knock couldn't save a copy of your Claude Code settings first, so it didn't change them. Please try again."
            }
            Self::Write => {
                "Knock couldn't save your Claude Code settings file. Nothing was changed. Please try again."
            }
            Self::NothingToRemove => {
                "Knock isn't in your Claude Code settings, so there was nothing to remove."
            }
            Self::Linked => {
                "Knock can't safely edit this settings file because it's linked from another place."
            }
            Self::RealFile => {
                "This is a development build of Knock, so it never changes your real Claude Code settings, even through a link. Point KNOCK_CLAUDE_SETTINGS at a test file."
            }
        }
    }
}

/// JSON that keeps object keys in file order. serde_json's `Value` sorts
/// them; its `preserve_order` feature would change key order for every
/// crate in the workspace build (replay output, adapter snapshots).
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(Number),
    Str(String),
    Arr(Vec<J>),
    /// Duplicate keys are kept as they are; lookups use the last one, as
    /// `JSON.parse` does.
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        let J::Obj(m) = self else { return None };
        m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn get_mut(&mut self, key: &str) -> Option<&mut J> {
        let J::Obj(m) = self else { return None };
        m.iter_mut().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn str(&self) -> Option<&str> {
        if let J::Str(s) = self { Some(s) } else { None }
    }
    fn arr(&self) -> Option<&[J]> {
        if let J::Arr(a) = self { Some(a) } else { None }
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = J;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_f64<E: de::Error>(self, n: f64) -> Result<J, E> {
                Number::from_f64(n)
                    .map(J::Num)
                    .ok_or_else(|| E::custom("not a number"))
            }
            fn visit_str<E>(self, s: &str) -> Result<J, E> {
                Ok(J::Str(s.into()))
            }
            fn visit_string<E>(self, s: String) -> Result<J, E> {
                Ok(J::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(J::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut m = Vec::new();
                while let Some(e) = a.next_entry()? {
                    m.push(e);
                }
                Ok(J::Obj(m))
            }
        }
        d.deserialize_any(V)
    }
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(x) => s.serialize_str(x),
            J::Arr(v) => s.collect_seq(v),
            J::Obj(m) => s.collect_map(m.iter().map(|(k, v)| (k, v))),
        }
    }
}

/// The exact §8.2 handler, keys in the §8.2 order.
fn handler(spec: &HookSpec, port: u16, token: &str) -> J {
    J::Obj(vec![
        ("type".into(), J::Str("http".into())),
        ("url".into(), J::Str(format!("{OWNER}{port}{}", spec.path))),
        (
            "headers".into(),
            J::Obj(vec![(TOKEN_HEADER.into(), J::Str(token.into()))]),
        ),
        ("timeout".into(), J::Num(1.into())),
    ])
}

fn is_knock(h: &J, spec: &HookSpec) -> bool {
    (h.get("url").and_then(J::str)).is_some_and(|u| u.starts_with(OWNER) && u.contains(spec.path))
}

/// Every handler in every matcher group of every event.
fn handlers(root: &J) -> impl Iterator<Item = &J> {
    let events: &[(String, J)] = match root.get("hooks") {
        Some(J::Obj(m)) => m,
        _ => &[],
    };
    (events.iter())
        .filter_map(|(_, groups)| groups.arr())
        .flatten()
        .filter_map(|g| g.get("hooks")?.arr())
        .flatten()
}

/// The file as JSON. No file (or no `.claude` folder) is `{}`. A leading
/// UTF-8 BOM is ignored (ADR 0010).
pub fn parse(raw: Option<&[u8]>) -> Result<J, Problem> {
    let Some(b) = raw else {
        return Ok(J::Obj(Vec::new()));
    };
    let v: J =
        serde_json::from_slice(b.strip_prefix(BOM).unwrap_or(b)).map_err(|_| Problem::NotJson)?;
    match v {
        J::Obj(_) => Ok(v),
        _ => Err(Problem::Shape),
    }
}

/// Removes Knock's handlers. A matcher group, an event array and then
/// `hooks` itself are removed only if Knock emptied them; with `keep`, the
/// adapter's own event arrays stay, so a re-install keeps their place.
/// Returns how many handlers were removed.
fn strip(root: &mut J, spec: &HookSpec, keep: bool) -> Result<usize, Problem> {
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(0);
    };
    let J::Obj(hooks) = hooks else {
        return Err(Problem::Shape);
    };
    let mut removed = 0;
    hooks.retain_mut(|(event, groups)| {
        let J::Arr(groups) = groups else {
            return true;
        };
        let mut here = 0;
        groups.retain_mut(|g| {
            let Some(J::Arr(hs)) = g.get_mut("hooks") else {
                return true;
            };
            let len = hs.len();
            hs.retain(|h| !is_knock(h, spec));
            here += len - hs.len();
            !(len > hs.len() && hs.is_empty())
        });
        removed += here;
        let ours = keep && spec.events.iter().any(|(e, _)| e == event);
        !(here > 0 && groups.is_empty() && !ours)
    });
    if removed > 0 && hooks.is_empty() && !keep {
        let J::Obj(m) = root else { unreachable!() };
        if let Some(i) = m.iter().rposition(|(k, _)| k == "hooks") {
            m.remove(i);
        }
    }
    Ok(removed)
}

/// Setup: Knock's old entries (any port or token) replaced by one handler
/// per event, appended to that event's array.
pub fn install(root: &mut J, spec: &HookSpec, port: u16, token: &str) -> Result<(), Problem> {
    strip(root, spec, true)?;
    let J::Obj(m) = root else {
        return Err(Problem::Shape);
    };
    if !m.iter().any(|(k, _)| k == "hooks") {
        m.push(("hooks".into(), J::Obj(Vec::new())));
    }
    let Some(J::Obj(hooks)) = root.get_mut("hooks") else {
        return Err(Problem::Shape);
    };
    for (event, matcher) in spec.events {
        if !hooks.iter().any(|(k, _)| k == event) {
            hooks.push((event.to_string(), J::Arr(Vec::new())));
        }
        let last = hooks.iter_mut().rev().find(|(k, _)| k == event);
        let Some((_, J::Arr(groups))) = last else {
            return Err(Problem::Shape);
        };
        let mut g = Vec::new();
        if let Some(m) = matcher {
            g.push(("matcher".into(), J::Str(m.to_string())));
        }
        g.push(("hooks".into(), J::Arr(vec![handler(spec, port, token)])));
        groups.push(J::Obj(g));
    }
    Ok(())
}

/// Removal: only Knock's entries.
pub fn uninstall(root: &mut J, spec: &HookSpec) -> Result<usize, Problem> {
    strip(root, spec, false)
}

/// Installed means: exactly one Knock handler per event, each the current
/// §8.2 handler (keys in any order) in a group with the right matcher, and
/// no other Knock entry.
pub fn installed(v: &J, spec: &HookSpec, port: u16, token: &str) -> bool {
    let value = |j: &J| serde_json::to_value(j).unwrap_or(Value::Null);
    let want = value(&handler(spec, port, token));
    let ours = handlers(v).filter(|h| is_knock(h, spec)).count();
    ours == spec.events.len()
        && spec.events.iter().all(|(event, matcher)| {
            let groups = v.get("hooks").and_then(|h| h.get(event)).and_then(J::arr);
            groups.into_iter().flatten().any(|g| {
                g.get("matcher").and_then(J::str) == *matcher
                    && (g.get("hooks").and_then(J::arr))
                        .is_some_and(|hs| hs.iter().any(|h| value(h) == want))
            })
        })
}

/// The original's indent unit: the leading whitespace of its first indented
/// line, if that is a tab or only spaces. Else (new, one-line or minified
/// file) 2 spaces.
fn indent(original: Option<&str>) -> &str {
    let mut lines = original.into_iter().flat_map(str::lines).skip(1);
    let first = lines.find_map(|l| {
        let rest = l.trim_start_matches([' ', '\t']);
        let lead = &l[..l.len() - rest.len()];
        (!lead.is_empty() && !rest.is_empty()).then_some(lead)
    });
    match first {
        Some(w) if w.starts_with('\t') => "\t",
        Some(w) if w.bytes().all(|b| b == b' ') => w,
        _ => "  ",
    }
}

/// `v` as the file text: pretty JSON in the original's indentation, with
/// its trailing newline (a new file gets one) and CRLF line ends if it used
/// them.
fn render(v: &J, original: Option<&str>) -> String {
    let mut out = Vec::new();
    let f = serde_json::ser::PrettyFormatter::with_indent(indent(original).as_bytes());
    let _ = v.serialize(&mut serde_json::Serializer::with_formatter(&mut out, f));
    let mut s = String::from_utf8(out).unwrap_or_default();
    if original.is_none_or(|o| o.ends_with('\n')) {
        s.push('\n');
    }
    if original.is_some_and(|o| o.contains("\r\n")) {
        s = s.replace('\n', "\r\n");
    }
    s
}

/// The current token and every token in Knock's old entries.
fn tokens(v: &J, spec: &HookSpec, token: &str) -> Vec<String> {
    let old = handlers(v)
        .filter(|h| is_knock(h, spec))
        .filter_map(|h| match h.get("headers") {
            Some(J::Obj(m)) => Some(m),
            _ => None,
        })
        .flatten()
        .filter(|(k, _)| k.eq_ignore_ascii_case(TOKEN_HEADER))
        .filter_map(|(_, t)| t.str().map(str::to_owned));
    let mut all: Vec<String> = std::iter::once(token.to_owned()).chain(old).collect();
    all.retain(|t| !t.is_empty());
    all
}

/// Replaces each token, as a JSON string, with `"••••"`.
fn mask(text: &str, tokens: &[String]) -> String {
    let masked = format!("\"{MASK}\"");
    tokens.iter().fold(text.to_owned(), |t, tok| {
        t.replace(&json!(tok).to_string(), &masked)
    })
}

/// What setup will write, from the file's bytes (`None`: no file).
pub struct Plan {
    /// The file text as read, without a BOM.
    pub before: String,
    /// Exactly the text that will be written.
    pub after: String,
    tokens: Vec<String>,
}

impl Plan {
    pub fn masked(&self) -> (String, String) {
        (
            mask(&self.before, &self.tokens),
            mask(&self.after, &self.tokens),
        )
    }
}

fn text(raw: Option<&[u8]>) -> Option<String> {
    raw.map(|b| String::from_utf8_lossy(b.strip_prefix(BOM).unwrap_or(b)).into_owned())
}

pub fn plan_install(
    raw: Option<&[u8]>,
    spec: &HookSpec,
    port: u16,
    token: &str,
) -> Result<Plan, Problem> {
    let mut v = parse(raw)?;
    let tokens = tokens(&v, spec, token);
    install(&mut v, spec, port, token)?;
    let before = text(raw);
    let after = render(&v, before.as_deref());
    Ok(Plan {
        before: before.unwrap_or_default(),
        after,
        tokens,
    })
}

pub fn plan_remove(raw: Option<&[u8]>, spec: &HookSpec) -> Result<Plan, Problem> {
    let mut v = parse(raw)?;
    let tokens = tokens(&v, spec, "");
    if uninstall(&mut v, spec)? == 0 {
        return Err(Problem::NothingToRemove);
    }
    let before = text(raw);
    let after = render(&v, before.as_deref());
    Ok(Plan {
        before: before.unwrap_or_default(),
        after,
        tokens,
    })
}

/// The file's bytes; `None` if it or its folder doesn't exist.
pub fn read(path: &Path) -> Result<Option<Vec<u8>>, Problem> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "claude settings unreadable");
            Err(Problem::Unreadable)
        }
    }
}

/// Copies `original` to `backups\settings.<UTC time>.json`, never over an
/// existing backup.
fn backup(dir: &Path, original: &[u8], now_ms: u64) -> io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let stamp =
        crate::log::utc(UNIX_EPOCH + Duration::from_millis(now_ms)).replace([':', '.'], "-");
    for n in 0..100 {
        let name = match n {
            0 => format!("settings.{stamp}.json"),
            _ => format!("settings.{stamp}-{n}.json"),
        };
        let path = dir.join(name);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        // A backup can hold API keys from the user's settings: owner only.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        match opts.open(&path) {
            Ok(mut f) => {
                f.write_all(original)?;
                f.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free backup name"))
}

/// Whether the file still holds exactly `original` (`None`: no file).
fn unchanged(path: &Path, original: Option<&[u8]>) -> bool {
    matches!(read(path), Ok(now) if now.as_deref() == original)
}

/// Backs up `original` (if there was a file), then writes `after`
/// atomically, without a BOM, if `still()` holds just before the replace
/// (else [`Problem::Stale`]; the backup stays). Returns the backup's path,
/// or `""` when there was no file to back up.
fn commit(
    path: &Path,
    backups: &Path,
    original: Option<&[u8]>,
    after: &str,
    now_ms: u64,
    still: impl FnOnce() -> bool,
) -> Result<String, Problem> {
    let saved = match original {
        Some(o) => backup(backups, o, now_ms)
            .map_err(|e| {
                tracing::warn!(kind = ?e.kind(), "claude settings backup failed");
                Problem::Backup
            })?
            .display()
            .to_string(),
        None => String::new(),
    };
    match crate::settings::write_atomic_if(path, after.as_bytes(), still) {
        Ok(true) => Ok(saved),
        Ok(false) => {
            tracing::info!("claude settings changed during the write; not written");
            Err(Problem::Stale)
        }
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "claude settings not written");
            Err(Problem::Write)
        }
    }
}

/// The file setup reads and writes for `path`: its final target if it or a
/// folder above it is a link, else `path` itself (also when there is no
/// file yet). A link to nothing is unreadable, so it is never replaced by a
/// file; a file with another hard link is [`Problem::Linked`]. A target
/// that is one of the `guard` files is [`Problem::RealFile`], decided
/// before the target is opened (ADR 0024).
pub fn target(path: &Path, guard: &[PathBuf]) -> Result<PathBuf, Problem> {
    let unreadable = |e: io::Error| {
        tracing::warn!(kind = ?e.kind(), "claude settings unreadable");
        Problem::Unreadable
    };
    let t = match std::fs::canonicalize(path) {
        Ok(t) => Some(t),
        Err(e)
            if e.kind() == io::ErrorKind::NotFound && std::fs::symlink_metadata(path).is_err() =>
        {
            None
        }
        Err(e) => return Err(unreadable(e)),
    };
    let named = resolved(t.as_deref().unwrap_or(path));
    if guard.iter().any(|g| resolved(g) == named) {
        tracing::info!("debug build: the real claude settings file; not edited");
        return Err(Problem::RealFile);
    }
    let Some(t) = t else {
        return Ok(path.to_owned());
    };
    let f = std::fs::File::open(&t).map_err(unreadable)?;
    match crate::platform::link_count(&f).map_err(unreadable)? {
        1 => Ok(t),
        _ => {
            tracing::info!("claude settings file has another hard link; not edited");
            Err(Problem::Linked)
        }
    }
}

/// `path` through every link, for comparing: canonical if it exists, else
/// its folder canonical plus its name, else as given; without the `\\?\`
/// prefix, lower-cased (Windows paths ignore case). Resolving opens no file
/// for reading.
fn resolved(path: &Path) -> String {
    let canon = std::fs::canonicalize(path).ok().or_else(|| {
        let dir = std::fs::canonicalize(path.parent()?).ok()?;
        Some(dir.join(path.file_name()?))
    });
    shown(canon.as_deref().unwrap_or(path)).to_lowercase()
}

/// `path` as the user knows it: without the `\\?\` prefix `canonicalize` adds.
fn shown(path: &Path) -> String {
    let s = path.display().to_string();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => s.strip_prefix(r"\\?\").map_or(s.clone(), str::to_owned),
    }
}

/// The preview the user is looking at.
struct Previewed {
    raw: Option<Vec<u8>>,
    after: String,
}

/// Hook setup for one settings file, with its backups folder, the server's
/// port and token, and the last preview. One operation at a time.
pub struct Setup {
    pub spec: HookSpec,
    /// `None` in a debug build without `KNOCK_CLAUDE_SETTINGS`.
    pub path: Option<PathBuf>,
    pub backups: PathBuf,
    /// Debug builds: the real Claude Code settings files, never edited even
    /// through a link (ADR 0024). Empty in release builds and tests.
    pub guard: Vec<PathBuf>,
    /// The running server's port and token; changed by Repair (ADR 0023).
    server: Mutex<Option<(u16, String)>>,
    previewed: Mutex<Option<Previewed>>,
}

pub struct Preview {
    pub path: String,
    pub before: String,
    pub after: String,
}

/// What the settings file holds of Knock's (FR-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hooks {
    /// Exactly the current hooks: this port and token.
    Installed,
    /// Knock's entries with another port or token: Claude Code still points
    /// at an old connection, for example after a Repair the user didn't
    /// confirm the Connect step for.
    Stale,
    /// No Knock entry, or some missing.
    Missing,
}

impl Setup {
    pub fn new(
        spec: HookSpec,
        path: Option<PathBuf>,
        backups: PathBuf,
        server: Option<(u16, String)>,
    ) -> Self {
        Self {
            spec,
            path,
            backups,
            guard: Vec::new(),
            server: Mutex::new(server),
            previewed: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Previewed>> {
        self.previewed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn path(&self) -> Result<PathBuf, Problem> {
        target(self.path.as_deref().ok_or(Problem::NoPath)?, &self.guard)
    }

    /// The running server's port and token, if it runs.
    pub fn server(&self) -> Option<(u16, String)> {
        self.server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// After Repair: the new port (ADR 0023). A preview made before it no
    /// longer matches, so apply asks for a new one.
    pub fn set_server(&self, server: Option<(u16, String)>) {
        *self.server.lock().unwrap_or_else(|e| e.into_inner()) = server;
    }

    fn running(&self) -> Result<(u16, String), Problem> {
        self.server().ok_or(Problem::NoServer)
    }

    /// The whole file before and after, token masked; remembered so apply
    /// can tell whether the file changed since.
    pub fn preview(&self) -> Result<Preview, Problem> {
        let mut last = self.lock();
        *last = None;
        let path = self.path()?;
        let (port, token) = self.running()?;
        let raw = read(&path)?;
        let plan = plan_install(raw.as_deref(), &self.spec, port, &token)?;
        let (before, after) = plan.masked();
        *last = Some(Previewed {
            raw,
            after: plan.after,
        });
        Ok(Preview {
            path: shown(&path),
            before,
            after,
        })
    }

    /// Writes what the last preview showed, re-read and re-computed now. If
    /// the file or the result differs from the preview, or the file changes
    /// before the replace: [`Problem::Stale`].
    pub fn apply(&self, now_ms: u64) -> Result<String, Problem> {
        let mut last = self.lock();
        let path = self.path()?;
        let (port, token) = self.running()?;
        let shown = last.take().ok_or(Problem::NoPreview)?;
        let raw = read(&path)?;
        if raw != shown.raw {
            return Err(Problem::Stale);
        }
        let plan = plan_install(raw.as_deref(), &self.spec, port, &token)?;
        if plan.after != shown.after {
            return Err(Problem::Stale);
        }
        let raw = raw.as_deref();
        let still = || unchanged(&path, raw);
        let saved = commit(&path, &self.backups, raw, &plan.after, now_ms, still)?;
        tracing::info!(events = self.spec.events.len(), "claude hooks installed");
        Ok(saved)
    }

    /// Deletes Knock's entries, with the same backup and atomic write.
    pub fn remove(&self, now_ms: u64) -> Result<String, Problem> {
        let mut last = self.lock();
        *last = None;
        let path = self.path()?;
        let raw = read(&path)?;
        let plan = plan_remove(raw.as_deref(), &self.spec)?;
        let raw = raw.as_deref();
        let still = || unchanged(&path, raw);
        let saved = commit(&path, &self.backups, raw, &plan.after, now_ms, still)?;
        tracing::info!("claude hooks removed");
        Ok(saved)
    }

    /// Whether the file holds exactly the current hooks (this port and
    /// token), Knock's entries for another connection, or neither. A file
    /// setup couldn't change gives the same problem setup would.
    pub fn status(&self) -> Result<Hooks, Problem> {
        let _last = self.lock();
        let (port, token) = self.running()?;
        let v = parse(read(&self.path()?)?.as_deref())?;
        install(&mut v.clone(), &self.spec, port, &token)?;
        if installed(&v, &self.spec, port, &token) {
            return Ok(Hooks::Installed);
        }
        let current = serde_json::to_value(handler(&self.spec, port, &token)).ok();
        let old = handlers(&v)
            .filter(|h| is_knock(h, &self.spec))
            .any(|h| serde_json::to_value(h).ok() != current);
        Ok(if old { Hooks::Stale } else { Hooks::Missing })
    }

    /// [`Setup::status`] is [`Hooks::Installed`].
    pub fn installed(&self) -> Result<bool, Problem> {
        self.status().map(|s| s == Hooks::Installed)
    }
}

/// The Claude Code settings files for `env`, the one Claude Code reads
/// first (§8.2, ADR 0022): `<CLAUDE_CONFIG_DIR>\settings.json` if that
/// variable is non-empty, then `~/.claude/settings.json` (`%USERPROFILE%`
/// on Windows, `$HOME` elsewhere).
fn claude_files(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    claude_files_for(cfg!(windows), env)
}

fn claude_files_for(windows: bool, env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let config = env("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty());
    let home = knock_core::paths::home_dir_for(windows, &env).map(|h| h.join(".claude"));
    (config.map(PathBuf::from).into_iter())
        .chain(home)
        .map(|d| d.join("settings.json"))
        .collect()
}

/// The settings file Knock edits. Release builds: the one Claude Code
/// reads (ADR 0022). Debug builds: only the file `KNOCK_CLAUDE_SETTINGS`
/// names, so a development run can never touch the real settings file.
pub fn claude_settings() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k);
    match cfg!(debug_assertions) {
        true => env("KNOCK_CLAUDE_SETTINGS").map(PathBuf::from),
        false => claude_files(env).into_iter().next(),
    }
}

/// [`Setup::guard`]: in a debug build, both real settings files.
pub fn guarded() -> Vec<PathBuf> {
    match cfg!(debug_assertions) {
        true => claude_files(|k| std::env::var_os(k)),
        false => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use knock_core::{Adapter, ClaudeCode};
    use std::sync::atomic::{AtomicU32, Ordering};

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PORT: u16 = 50123;
    const T: u64 = 1_792_166_400_000;

    fn spec() -> HookSpec {
        ClaudeCode { users: vec![] }.hooks()
    }

    /// A fresh temp folder with `settings.json` (not created) and `backups`.
    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let d = std::env::temp_dir().join(format!("knock-hooks-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }
        fn file(&self) -> PathBuf {
            self.0.join("dot-claude").join("settings.json")
        }
        fn write(&self, bytes: &[u8]) {
            std::fs::create_dir_all(self.file().parent().unwrap()).unwrap();
            std::fs::write(self.file(), bytes).unwrap();
        }
        fn read(&self) -> String {
            std::fs::read_to_string(self.file()).unwrap()
        }
        fn setup(&self) -> Setup {
            Setup::new(
                spec(),
                Some(self.file()),
                self.0.join("backups"),
                Some((PORT, TOKEN.into())),
            )
        }
        fn backups(&self) -> Vec<PathBuf> {
            let Ok(rd) = std::fs::read_dir(self.0.join("backups")) else {
                return vec![];
            };
            let mut v: Vec<_> = rd.map(|e| e.unwrap().path()).collect();
            v.sort();
            v
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A realistic file, formatted the way Claude Code writes it: other
    /// keys, permissions, env, a command hook, another tool's HTTP hook, a
    /// group shared with a Knock entry, and an old Knock entry on another
    /// port with another token.
    const REAL: &str = r#"{
  "$schema": "https://json.schemastore.org/claude-code-settings.json",
  "model": "opus",
  "permissions": {
    "allow": [
      "Bash(npm run test:*)",
      "Read(~/.zshrc)"
    ],
    "deny": [
      "Read(./.env)"
    ],
    "defaultMode": "acceptEdits"
  },
  "env": {
    "FOO": "bar",
    "NUM": "1"
  },
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "echo hi",
            "timeout": 30
          }
        ]
      },
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "http",
            "url": "http://127.0.0.1:49999/v1/hooks/claude-code",
            "headers": {
              "X-Knock-Token": "oldtokenoldtokenoldtokenoldtokenoldtokenold"
            },
            "timeout": 2
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "http",
            "url": "http://localhost:8080/other-tool",
            "timeout": 5
          },
          {
            "type": "http",
            "url": "http://127.0.0.1:49999/v1/hooks/claude-code",
            "headers": {
              "X-Knock-Token": "oldtokenoldtokenoldtokenoldtokenoldtokenold"
            },
            "timeout": 2
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "http",
            "url": "http://127.0.0.1:49999/v1/hooks/claude-code",
            "timeout": 2
          }
        ]
      }
    ],
    "PostToolUseFailure": [
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "notify.exe",
            "async": true
          }
        ]
      }
    ]
  },
  "zeta": [
    1,
    2.5,
    -3e-7,
    null,
    true,
    "é \"q\" \\ \t"
  ],
  "alpha": {}
}
"#;

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    /// Order-keeping parse, for key-order checks.
    fn j(s: &str) -> J {
        serde_json::from_str(s).unwrap()
    }

    fn jv(v: Value) -> J {
        serde_json::from_value(v).unwrap()
    }

    fn val(x: &J) -> Value {
        serde_json::to_value(x).unwrap()
    }

    fn keys(x: &J) -> Vec<&str> {
        let J::Obj(m) = x else {
            panic!("not an object")
        };
        m.iter().map(|(k, _)| k.as_str()).collect()
    }

    fn hook_keys(text: &str) -> Vec<String> {
        let x = j(text);
        keys(x.get("hooks").unwrap())
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn is_ours(h: &Value) -> bool {
        h["url"]
            .as_str()
            .is_some_and(|u| u.starts_with(OWNER) && u.contains("/v1/hooks/claude-code"))
    }

    fn knock_handler(port: u16, token: &str) -> Value {
        json!({
            "type": "http",
            "url": format!("http://127.0.0.1:{port}/v1/hooks/claude-code"),
            "headers": { "X-Knock-Token": token },
            "timeout": 1
        })
    }

    /// Every Knock handler in `v`, as (event, matcher, handler).
    fn knock_entries(v: &Value) -> Vec<(String, Option<String>, Value)> {
        let mut out = vec![];
        for (event, groups) in v["hooks"].as_object().into_iter().flatten() {
            for g in groups.as_array().into_iter().flatten() {
                for h in g["hooks"].as_array().into_iter().flatten() {
                    if is_ours(h) {
                        let m = g.get("matcher").and_then(Value::as_str).map(str::to_owned);
                        out.push((event.clone(), m, h.clone()));
                    }
                }
            }
        }
        out
    }

    /// Exactly the §8.2 list: 11 events, tool events with "*", no
    /// SessionStart (ADR 0005), StopFailure (ADR 0007), the exact handler.
    fn assert_exact_hooks(v: &Value) {
        let want: Vec<(String, Option<String>, Value)> = [
            ("UserPromptSubmit", None),
            ("PreToolUse", Some("*")),
            ("PostToolUse", Some("*")),
            ("PermissionRequest", Some("*")),
            ("Notification", None),
            ("Stop", None),
            ("StopFailure", None),
            ("SubagentStart", None),
            ("SubagentStop", None),
            ("PreCompact", None),
            ("SessionEnd", None),
        ]
        .iter()
        .map(|(e, m)| {
            (
                e.to_string(),
                m.map(str::to_owned),
                knock_handler(PORT, TOKEN),
            )
        })
        .collect();
        let mut got = knock_entries(v);
        let mut want = want;
        got.sort_by(|a, b| a.0.cmp(&b.0));
        want.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got, want);
        assert!(installed(&jv(v.clone()), &spec(), PORT, TOKEN));
    }

    /// `v` without Knock's entries, and without what Knock emptied.
    fn without_knock(v: Value) -> Value {
        let mut x = jv(v);
        uninstall(&mut x, &spec()).unwrap();
        val(&x)
    }

    #[test]
    fn the_adapter_lists_the_spec_events() {
        let s = spec();
        assert_eq!(s.path, "/v1/hooks/claude-code");
        assert_eq!(s.events.len(), 11);
        assert!(!s.events.iter().any(|(e, _)| *e == "SessionStart"));
    }

    #[test]
    fn missing_file_and_folder_become_a_new_file_with_exactly_the_hooks() {
        let d = Dir::new();
        let s = d.setup();
        let p = s.preview().unwrap();
        assert_eq!(p.before, "");
        assert!(p.path.ends_with("settings.json"));
        assert_eq!(s.apply(T).unwrap(), "", "nothing to back up");
        let text = d.read();
        assert!(text.ends_with("}\n") && !text.starts_with('\u{feff}'));
        let got = v(&text);
        assert_exact_hooks(&got);
        assert_eq!(got.as_object().unwrap().len(), 1, "only hooks");
        assert!(d.backups().is_empty());
        // The preview showed exactly what was written, masked.
        assert_eq!(p.after, text.replace(TOKEN, MASK));
        assert!(!p.after.contains(TOKEN));
    }

    #[test]
    fn preview_masks_every_token_and_is_exactly_what_apply_writes() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        let p = s.preview().unwrap();
        assert_eq!(
            p.before,
            REAL.replace("oldtokenoldtokenoldtokenoldtokenoldtokenold", MASK)
        );
        for t in [TOKEN, "oldtokenoldtokenoldtokenoldtokenoldtokenold"] {
            assert!(!p.before.contains(t) && !p.after.contains(t));
        }
        assert_eq!(
            std::fs::read_to_string(d.file()).unwrap(),
            REAL,
            "preview writes nothing"
        );
        assert!(d.backups().is_empty());
        s.apply(T).unwrap();
        assert_eq!(p.after, d.read().replace(TOKEN, MASK));
    }

    #[test]
    fn apply_preserves_every_other_key_value_and_order() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        let text = d.read();
        let got = v(&text);
        assert_exact_hooks(&got);
        // The old port is gone; the other tool's and the user's hooks stay.
        assert!(!text.contains("49999"));
        assert!(text.contains("http://localhost:8080/other-tool"));
        assert_eq!(without_knock(got.clone()), without_knock(v(REAL)));
        // Top-level key order is kept, values unchanged.
        assert_eq!(
            keys(&j(&text)),
            [
                "$schema",
                "model",
                "permissions",
                "env",
                "hooks",
                "zeta",
                "alpha"
            ]
        );
        for k in ["$schema", "model", "permissions", "env", "zeta", "alpha"] {
            assert_eq!(got[k], v(REAL)[k], "{k}");
        }
        // Event order: the user's events first, in place; the old Knock-only
        // SessionStart array is removed because Knock emptied it.
        let events = hook_keys(&text);
        assert_eq!(&events[..3], ["PreToolUse", "Stop", "PostToolUseFailure"]);
        assert_eq!(
            events.len(),
            3 + 9,
            "PreToolUse and Stop were already there"
        );
        assert!(got["hooks"].get("SessionStart").is_none());
        // The user's PreToolUse group is untouched and first.
        assert_eq!(
            got["hooks"]["PreToolUse"][0],
            v(REAL)["hooks"]["PreToolUse"][0]
        );
        // The shared Stop group keeps the other tool's handler only.
        assert_eq!(
            got["hooks"]["Stop"][0]["hooks"],
            json!([{ "type": "http", "url": "http://localhost:8080/other-tool", "timeout": 5 }])
        );
        // Formatting: everything outside "hooks" is byte-identical.
        let tail = |t: &str| t[t.find("\n  \"zeta\"").unwrap()..].to_owned();
        assert_eq!(tail(&text), tail(REAL));
        let head = |t: &str| t[..t.find("\n  \"hooks\"").unwrap()].to_owned();
        assert_eq!(head(&text), head(REAL));
    }

    #[test]
    fn apply_is_idempotent_and_replaces_an_old_port() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        let once = d.read();
        let p = s.preview().unwrap();
        assert_eq!(p.before, p.after, "a re-run changes nothing");
        s.apply(T + 1).unwrap();
        assert_eq!(d.read(), once);
        assert_eq!(knock_entries(&v(&once)).len(), 11, "never duplicated");

        // Knock moved to another port: the old entries are replaced, and the
        // events keep their place.
        let moved = Setup::new(
            spec(),
            Some(d.file()),
            d.0.join("backups"),
            Some((61000, "new".into())),
        );
        moved.preview().unwrap();
        moved.apply(T + 2).unwrap();
        let got = v(&d.read());
        let entries = knock_entries(&got);
        assert_eq!(entries.len(), 11);
        assert!(
            entries
                .iter()
                .all(|(_, _, h)| *h == knock_handler(61000, "new"))
        );
        assert!(!d.read().contains(&PORT.to_string()));
        assert_eq!(
            hook_keys(&d.read()),
            hook_keys(&once),
            "event order kept on re-install"
        );
    }

    #[test]
    fn remove_deletes_only_knock_and_what_knock_emptied() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        let installed_text = d.read();
        let backup = s.remove(T + 1).unwrap();
        let got = v(&d.read());
        assert!(knock_entries(&got).is_empty());
        assert_eq!(got, without_knock(v(REAL)));
        // The events Knock added are gone; the user's stay, in order.
        let events = hook_keys(&d.read());
        assert_eq!(events, ["PreToolUse", "Stop", "PostToolUseFailure"]);
        assert_eq!(keys(&j(&d.read())), keys(&j(REAL)));
        // The backup holds the file as it was just before the removal.
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), installed_text);
        assert_eq!(s.remove(T + 2), Err(Problem::NothingToRemove));
        assert_eq!(d.backups().len(), 2, "no write when nothing to remove");
    }

    #[test]
    fn remove_after_install_on_a_new_file_leaves_an_empty_object() {
        let d = Dir::new();
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        s.remove(T + 1).unwrap();
        assert_eq!(d.read(), "{}\n");
    }

    #[test]
    fn remove_keeps_groups_and_arrays_knock_did_not_empty() {
        let mut x = jv(json!({ "hooks": {
            "Stop": [],
            "Notification": [{ "hooks": [] }],
            "PreToolUse": [{ "matcher": "*", "hooks": [knock_handler(PORT, TOKEN)] }],
        }}));
        assert_eq!(uninstall(&mut x, &spec()), Ok(1));
        assert_eq!(
            val(&x),
            json!({ "hooks": { "Stop": [], "Notification": [{ "hooks": [] }] } })
        );
        // Not Knock's: other hosts, other paths, other schemes.
        for url in [
            "http://localhost:50123/v1/hooks/claude-code",
            "https://127.0.0.1:50123/v1/hooks/claude-code",
            "http://127.0.0.1:50123/v1/hooks/other",
            "http://127.0.0.2:50123/v1/hooks/claude-code",
        ] {
            let mut y =
                jv(json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "http", "url": url }] }] } }));
            assert_eq!(uninstall(&mut y, &spec()), Ok(0), "{url}");
        }
    }

    #[test]
    fn invalid_json_or_shape_writes_nothing() {
        for (bad, problem) in [
            (&b"{ \"hooks\": "[..], Problem::NotJson),
            (b"", Problem::NotJson),
            (b"{ // comment\n}", Problem::NotJson),
            (b"\xFF\xFE{}", Problem::NotJson),
            (b"[]", Problem::Shape),
            (b"\"text\"", Problem::Shape),
            (b"{\"hooks\": []}", Problem::Shape),
            (b"{\"hooks\": {\"Stop\": {}}}", Problem::Shape),
        ] {
            let d = Dir::new();
            d.write(bad);
            let s = d.setup();
            assert_eq!(s.preview().err(), Some(problem), "{bad:?}");
            assert_eq!(s.apply(T), Err(Problem::NoPreview));
            // Removal ignores an event it can't read: nothing of Knock's is in it.
            let removed = s.remove(T).err();
            assert!(removed == Some(problem) || removed == Some(Problem::NothingToRemove));
            assert_eq!(s.installed(), Err(problem));
            assert_eq!(std::fs::read(d.file()).unwrap(), bad);
            assert!(d.backups().is_empty());
        }
        let m = Problem::NotJson.message();
        assert!(m.starts_with(
            "Your Claude Code settings file isn't valid JSON, so Knock didn't change it."
        ));
    }

    #[test]
    fn a_bom_is_ignored_on_read_kept_in_the_backup_and_not_written() {
        let d = Dir::new();
        let mut bytes = BOM.to_vec();
        bytes.extend(br#"{"model": "opus"}"#);
        d.write(&bytes);
        let s = d.setup();
        let p = s.preview().unwrap();
        assert_eq!(p.before, r#"{"model": "opus"}"#);
        let backup = s.apply(T).unwrap();
        let written = std::fs::read(d.file()).unwrap();
        assert!(!written.starts_with(BOM));
        assert_eq!(v(std::str::from_utf8(&written).unwrap())["model"], "opus");
        // The backup can hold API keys: owner only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&backup).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(std::fs::read(backup).unwrap(), bytes);
    }

    #[test]
    fn crlf_and_no_trailing_newline_are_kept() {
        let d = Dir::new();
        d.write(b"{\r\n  \"model\": \"opus\"\r\n}");
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        let text = d.read();
        assert!(text.starts_with("{\r\n  \"model\": \"opus\",\r\n  \"hooks\": {\r\n"));
        assert!(!text.contains("\r\r") && !text.replace("\r\n", "").contains('\n'));
        assert!(text.ends_with('}'));
    }

    /// `text` with each leading 2-space step replaced by `unit`.
    fn reindent(text: &str, unit: &str) -> String {
        (text.split_inclusive('\n'))
            .map(|l| {
                let n = l.len() - l.trim_start_matches(' ').len();
                format!("{}{}", unit.repeat(n / 2), &l[n..])
            })
            .collect()
    }

    fn install_into(d: &Dir, bytes: &[u8]) -> String {
        d.write(bytes);
        let s = d.setup();
        s.preview().unwrap();
        s.apply(T).unwrap();
        d.read()
    }

    #[test]
    fn the_files_own_indentation_is_kept() {
        let two = install_into(&Dir::new(), REAL.as_bytes());
        for unit in ["\t", "    ", "  "] {
            let original = reindent(REAL, unit);
            let text = install_into(&Dir::new(), original.as_bytes());
            // Knock's entries in the same indentation as the rest.
            assert_eq!(text, reindent(&two, unit), "{unit:?}");
            // Byte-identical outside "hooks".
            let head = |t: &str| t[..t.find(&format!("\n{unit}\"hooks\"")).unwrap()].to_owned();
            let tail = |t: &str| t[t.find(&format!("\n{unit}\"zeta\"")).unwrap()..].to_owned();
            assert_eq!(head(&text), head(&original), "{unit:?}");
            assert_eq!(tail(&text), tail(&original), "{unit:?}");
        }
        // Tabs with CRLF.
        let crlf = reindent(REAL, "\t").replace('\n', "\r\n");
        let text = install_into(&Dir::new(), crlf.as_bytes());
        assert_eq!(text, reindent(&two, "\t").replace('\n', "\r\n"));
        // One line or minified: 2 spaces.
        let text = install_into(&Dir::new(), br#"{"model":"opus"}"#);
        assert!(text.starts_with("{\n  \"model\": \"opus\",\n  \"hooks\": {\n    \""));
        assert_eq!(indent(None), "  ");
        assert_eq!(indent(Some("{\n\n   \"a\": 1\n}")), "   ");
        assert_eq!(indent(Some("{\n \t\"a\": 1\n}")), "  ", "mixed");
    }

    /// A symbolic link to a file. On Windows it needs Developer Mode or
    /// admin rights, so callers skip their link case when it fails.
    fn symlink_file(real: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(windows)]
        return std::os::windows::fs::symlink_file(real, link);
        #[cfg(unix)]
        return std::os::unix::fs::symlink(real, link);
    }

    #[test]
    fn a_symlinked_file_is_read_backed_up_and_written_at_its_target() {
        let d = Dir::new();
        let real = d.0.join("dotfiles").join("claude-settings.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::create_dir_all(d.file().parent().unwrap()).unwrap();
        std::fs::write(&real, REAL).unwrap();
        if let Err(e) = symlink_file(&real, &d.file()) {
            // Needs Developer Mode or admin rights; `target` is still
            // covered by the hard-link and missing-file tests.
            eprintln!("skipped: can't create a symlink here ({:?})", e.kind());
            return;
        }
        let is_link = || {
            std::fs::symlink_metadata(d.file())
                .unwrap()
                .file_type()
                .is_symlink()
        };
        let s = d.setup();
        let p = s.preview().unwrap();
        assert_eq!(p.path, shown(&std::fs::canonicalize(&real).unwrap()));
        assert!(
            Path::new(&p.path).ends_with(Path::new("dotfiles").join("claude-settings.json")),
            "{}",
            p.path
        );
        assert!(!p.path.starts_with(r"\\?\"));
        let backup = s.apply(T).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), REAL);
        assert!(is_link(), "the link stays a link");
        let written = std::fs::read_to_string(&real).unwrap();
        assert_exact_hooks(&v(&written));
        assert_eq!(d.read(), written, "read through the link");
        s.remove(T + 1).unwrap();
        assert!(is_link());
        assert_eq!(
            v(&std::fs::read_to_string(&real).unwrap()),
            without_knock(v(REAL))
        );
        for dir in [real.parent().unwrap(), d.file().parent().unwrap()] {
            assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1, "no temp file");
        }
        // A link to nothing is not "no file": nothing replaces the link.
        std::fs::remove_file(&real).unwrap();
        assert_eq!(s.preview().err(), Some(Problem::Unreadable));
        assert_eq!(s.remove(T + 2), Err(Problem::Unreadable));
        assert!(is_link() && !real.exists());
    }

    #[test]
    fn a_file_with_another_hard_link_is_refused_and_untouched() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let other = d.0.join("other-name.json");
        std::fs::hard_link(d.file(), &other).unwrap();
        let s = d.setup();
        assert_eq!(s.preview().err(), Some(Problem::Linked));
        assert_eq!(s.apply(T), Err(Problem::Linked));
        assert_eq!(s.remove(T), Err(Problem::Linked));
        assert_eq!(s.installed(), Err(Problem::Linked));
        assert_eq!(d.read(), REAL);
        assert!(d.backups().is_empty());
        assert_eq!(
            Problem::Linked.message(),
            "Knock can't safely edit this settings file because it's linked from another place."
        );
        // One name again: setup works.
        std::fs::remove_file(&other).unwrap();
        s.preview().unwrap();
        s.apply(T).unwrap();
        // A file that doesn't exist yet is written where it is named.
        assert_eq!(
            target(&d.0.join("none.json"), &[]),
            Ok(d.0.join("none.json"))
        );
    }

    #[test]
    fn a_change_between_the_backup_and_the_replace_aborts_and_keeps_the_backup() {
        let d = Dir::new();
        d.write(b"{}");
        let (path, backups) = (d.file(), d.0.join("backups"));
        let sonnet = br#"{"model":"sonnet"}"#;
        // Claude Code writes the file after the backup, before the rename.
        let still = || {
            d.write(sonnet);
            unchanged(&path, Some(b"{}"))
        };
        let r = commit(&path, &backups, Some(b"{}"), "{\"x\": 1}\n", T, still);
        assert_eq!(r, Err(Problem::Stale));
        assert_eq!(d.read().as_bytes(), sonnet, "Claude Code's change kept");
        let kept = d.backups();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(&kept[0]).unwrap(), b"{}");
        let left = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(left, 1, "the temp file, which holds the token, is gone");
        // A file that appears in between, too.
        std::fs::remove_file(&path).unwrap();
        let still = || {
            d.write(b"{}");
            unchanged(&path, None)
        };
        let r = commit(&path, &backups, None, "{\"x\": 1}\n", T, still);
        assert_eq!(r, Err(Problem::Stale));
        assert_eq!(d.read(), "{}");
        // Unchanged: written.
        let r = commit(&path, &backups, Some(b"{}"), "{\"x\": 1}\n", T, || {
            unchanged(&path, Some(b"{}"))
        });
        assert!(r.is_ok());
        assert_eq!(d.read(), "{\"x\": 1}\n");
    }

    #[test]
    fn apply_needs_a_fresh_preview_and_detects_a_changed_file() {
        let d = Dir::new();
        d.write(br#"{"model": "opus"}"#);
        let s = d.setup();
        assert_eq!(s.apply(T), Err(Problem::NoPreview));
        s.preview().unwrap();
        // Claude Code (or the user) changes the file after the preview.
        d.write(br#"{"model": "sonnet"}"#);
        assert_eq!(s.apply(T), Err(Problem::Stale));
        assert_eq!(d.read(), r#"{"model": "sonnet"}"#);
        assert_eq!(
            s.apply(T),
            Err(Problem::NoPreview),
            "a preview is used once"
        );
        // A file that appears after a "no file" preview is a change too.
        let e = Dir::new();
        let s = e.setup();
        s.preview().unwrap();
        e.write(b"{}");
        assert_eq!(s.apply(T), Err(Problem::Stale));
        // A file deleted after the preview, too.
        let f = Dir::new();
        f.write(b"{}");
        let s = f.setup();
        s.preview().unwrap();
        std::fs::remove_file(f.file()).unwrap();
        assert_eq!(s.apply(T), Err(Problem::Stale));
        assert!(!f.file().exists());
        assert_eq!(
            Problem::Stale.message(),
            "Your settings changed since the preview; please review again."
        );
        // A remove in between drops the preview.
        let g = Dir::new();
        let s = g.setup();
        s.preview().unwrap();
        assert_eq!(s.remove(T), Err(Problem::NothingToRemove));
        assert_eq!(s.apply(T), Err(Problem::NoPreview));
    }

    #[test]
    fn backups_hold_the_exact_original_and_never_overwrite() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        s.preview().unwrap();
        let b1 = s.apply(T).unwrap();
        assert_eq!(std::fs::read_to_string(&b1).unwrap(), REAL);
        let name = Path::new(&b1)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(name, "settings.2026-10-16T16-00-00-000Z.json");
        assert_eq!(Path::new(&b1).parent().unwrap(), d.0.join("backups"));
        let after1 = d.read();
        // Same millisecond: a new name, the first backup untouched.
        let b2 = s.remove(T).unwrap();
        assert_ne!(b1, b2);
        assert!(b2.ends_with("settings.2026-10-16T16-00-00-000Z-1.json"));
        assert_eq!(std::fs::read_to_string(&b1).unwrap(), REAL);
        assert_eq!(std::fs::read_to_string(&b2).unwrap(), after1);
        // No temp file is left next to the settings file.
        let left: Vec<_> = std::fs::read_dir(d.file().parent().unwrap())
            .unwrap()
            .collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn a_failed_backup_writes_nothing() {
        let d = Dir::new();
        d.write(b"{}");
        // A file where the backups folder should be.
        std::fs::write(d.0.join("backups"), b"x").unwrap();
        let s = d.setup();
        s.preview().unwrap();
        assert_eq!(s.apply(T), Err(Problem::Backup));
        assert_eq!(d.read(), "{}");
    }

    #[test]
    fn installed_only_with_the_exact_current_hooks() {
        let spec = spec();
        let mut x = jv(json!({}));
        assert!(!installed(&x, &spec, PORT, TOKEN));
        install(&mut x, &spec, PORT, TOKEN).unwrap();
        assert!(installed(&x, &spec, PORT, TOKEN));
        assert!(!installed(&x, &spec, PORT + 1, TOKEN));
        assert!(!installed(&x, &spec, PORT, "other"));
        // Key order inside the handler doesn't matter (sorted by Value).
        let x = jv(val(&x));
        assert!(installed(&x, &spec, PORT, TOKEN));
        let x = val(&x);
        let check = |v: Value| installed(&jv(v), &spec, PORT, TOKEN);
        // A duplicate Knock entry, or a missing one, is not installed.
        let mut dup = x.clone();
        dup["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "hooks": [knock_handler(PORT, TOKEN)] }));
        assert!(!check(dup));
        let mut gone = x.clone();
        gone["hooks"].as_object_mut().unwrap().remove("Stop");
        assert!(!check(gone));
        // A tool event without its "*" matcher is not installed.
        let mut nomatch = x.clone();
        nomatch["hooks"]["PreToolUse"][0]
            .as_object_mut()
            .unwrap()
            .remove("matcher");
        assert!(!check(nomatch));
        // Through Setup, from a file.
        let d = Dir::new();
        let s = d.setup();
        assert_eq!(s.installed(), Ok(false));
        s.preview().unwrap();
        s.apply(T).unwrap();
        assert_eq!(s.installed(), Ok(true));
    }

    #[test]
    fn no_path_or_no_server_touches_nothing() {
        let d = Dir::new();
        let s = Setup::new(
            spec(),
            None,
            d.0.join("backups"),
            Some((PORT, TOKEN.into())),
        );
        assert_eq!(s.preview().err(), Some(Problem::NoPath));
        assert_eq!(s.apply(T), Err(Problem::NoPath));
        assert_eq!(s.remove(T), Err(Problem::NoPath));
        assert_eq!(s.installed(), Err(Problem::NoPath));
        let s = Setup::new(spec(), Some(d.file()), d.0.join("backups"), None);
        assert_eq!(s.preview().err(), Some(Problem::NoServer));
        assert!(!d.file().exists());
    }

    #[test]
    fn entries_for_an_old_port_or_token_are_stale_not_installed() {
        let d = Dir::new();
        d.write(REAL.as_bytes());
        let s = d.setup();
        // REAL has Knock entries for port 49999 with an old token.
        assert_eq!(s.status(), Ok(Hooks::Stale));
        s.preview().unwrap();
        s.apply(T).unwrap();
        assert_eq!(
            (s.status(), s.installed()),
            (Ok(Hooks::Installed), Ok(true))
        );
        // Repair moved the server; the user stopped before Confirm.
        s.set_server(Some((PORT + 1, TOKEN.into())));
        assert_eq!((s.status(), s.installed()), (Ok(Hooks::Stale), Ok(false)));
        s.set_server(Some((PORT, "B".repeat(43))));
        assert_eq!(s.status(), Ok(Hooks::Stale), "another token");
        // A preview from before the repair can't be applied after it.
        s.set_server(Some((PORT, TOKEN.into())));
        s.preview().unwrap();
        s.set_server(Some((PORT + 1, TOKEN.into())));
        assert_eq!(s.apply(T + 1), Err(Problem::Stale));
        // Setup again: installed for the new port.
        s.preview().unwrap();
        s.apply(T + 2).unwrap();
        assert_eq!(s.status(), Ok(Hooks::Installed));
        // No Knock entry at all is missing, not stale.
        s.remove(T + 3).unwrap();
        assert_eq!(s.status(), Ok(Hooks::Missing));
        let e = Dir::new();
        assert_eq!(e.setup().status(), Ok(Hooks::Missing), "no file");
    }

    #[test]
    fn the_settings_file_follows_claude_config_dir_when_it_is_set() {
        fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
            move |k| {
                (vars.iter())
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        }
        let home = [("USERPROFILE", r"C:\Users\A")];
        assert_eq!(
            claude_files_for(true, env(&home)),
            [PathBuf::from(r"C:\Users\A")
                .join(".claude")
                .join("settings.json")]
        );
        let both = [
            ("USERPROFILE", r"C:\Users\A"),
            ("CLAUDE_CONFIG_DIR", r"D:\cfg\claude"),
        ];
        assert_eq!(
            claude_files_for(true, env(&both)),
            [
                PathBuf::from(r"D:\cfg\claude").join("settings.json"),
                PathBuf::from(r"C:\Users\A")
                    .join(".claude")
                    .join("settings.json"),
            ],
            "Claude Code's own first; both are guarded"
        );
        let empty = [("USERPROFILE", r"C:\Users\A"), ("CLAUDE_CONFIG_DIR", "")];
        assert_eq!(
            claude_files_for(true, env(&empty)),
            claude_files_for(true, env(&home))
        );
        let only = [("CLAUDE_CONFIG_DIR", r"D:\cfg")];
        assert_eq!(
            claude_files_for(true, env(&only)),
            [PathBuf::from(r"D:\cfg").join("settings.json")]
        );
        assert!(claude_files_for(true, env(&[])).is_empty());
        // Linux: $HOME, not %USERPROFILE%.
        let linux = [("HOME", "/home/a"), ("USERPROFILE", r"C:\Users\A")];
        assert_eq!(
            claude_files_for(false, env(&linux)),
            [PathBuf::from("/home/a/.claude/settings.json")]
        );
    }

    #[test]
    fn a_debug_guard_refuses_the_real_file_even_through_a_link() {
        let d = Dir::new();
        // A stand-in for the real file; its content must stay unread and unchanged.
        let real = d.0.join("Fake Home").join(".claude").join("settings.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, REAL).unwrap();
        let guarded = |path: PathBuf| {
            let mut s = Setup::new(
                spec(),
                Some(path),
                d.0.join("backups"),
                Some((PORT, TOKEN.into())),
            );
            s.guard = vec![real.clone()];
            s
        };
        let refused = |s: &Setup| {
            assert_eq!(s.preview().err(), Some(Problem::RealFile));
            assert_eq!(s.apply(T), Err(Problem::RealFile));
            assert_eq!(s.remove(T), Err(Problem::RealFile));
            assert_eq!(s.installed(), Err(Problem::RealFile));
        };
        // The same path, and the same path in other letter case.
        refused(&guarded(real.clone()));
        let upper = PathBuf::from(real.display().to_string().to_uppercase());
        refused(&guarded(upper));
        // Through a symbolic link (needs Developer Mode or admin rights).
        let link = d.0.join("scratch-settings.json");
        match symlink_file(&real, &link) {
            Ok(()) => refused(&guarded(link)),
            Err(e) => eprintln!("link case skipped: can't create a symlink ({:?})", e.kind()),
        }
        assert_eq!(std::fs::read_to_string(&real).unwrap(), REAL);
        assert!(d.backups().is_empty());
        // A real file that doesn't exist yet is guarded by name too.
        let absent = d.0.join("Other Home").join(".claude").join("settings.json");
        let mut s = guarded(absent.clone());
        s.guard = vec![absent.clone()];
        refused(&s);
        assert!(!absent.exists());
        // Any other file is fine.
        let s = guarded(d.file());
        s.preview().unwrap();
        s.apply(T).unwrap();
        assert_eq!(s.status(), Ok(Hooks::Installed));
    }

    #[test]
    fn messages_hold_no_paths_or_tokens() {
        use Problem::*;
        for p in [
            NoPath,
            NoServer,
            Unreadable,
            NotJson,
            Shape,
            NoPreview,
            Stale,
            Backup,
            Write,
            NothingToRemove,
            Linked,
            RealFile,
        ] {
            let m = p.message();
            assert!(
                !m.contains('\\') && !m.contains(TOKEN) && m.ends_with('.'),
                "{p:?}"
            );
        }
    }

    /// §11.5: setup logs never hold the file, a token or a path.
    #[test]
    fn logs_hold_no_content_token_or_path() {
        let d = Dir::new();
        let logs = d.0.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        {
            let _log = tracing::subscriber::set_default(crate::log::FileLog::new(logs.clone()));
            d.write(REAL.as_bytes());
            let s = d.setup();
            s.preview().unwrap();
            s.apply(T).unwrap();
            s.remove(T + 1).unwrap();
            d.write(b"{ \"model\": \"secret-model\", ");
            let _ = s.preview();
            // A file where the backups folder should be.
            std::fs::remove_dir_all(d.0.join("backups")).unwrap();
            std::fs::write(d.0.join("backups"), b"x").unwrap();
            d.write(b"{}");
            s.preview().unwrap();
            assert_eq!(s.apply(T + 2), Err(Problem::Backup));
        }
        let mut text = String::new();
        for e in std::fs::read_dir(&logs).unwrap() {
            text += &std::fs::read_to_string(e.unwrap().path()).unwrap();
        }
        // Which lines arrive depends on other test threads (tracing caches
        // per call site), so only what must never appear is checked.
        let tmp = std::env::temp_dir().display().to_string();
        for bad in [
            TOKEN,
            "oldtoken",
            "secret-model",
            "opus",
            "dot-claude",
            "settings.20",
            "\\\\",
            &tmp,
        ] {
            assert!(!text.contains(bad), "{bad} in {text}");
        }
    }
}
