//! File logging (SPEC.md §11.5): `knock.YYYY-MM-DD.log` in
//! `%LOCALAPPDATA%\Knock\logs` (Linux: `~/.local/state/knock/logs`),
//! one file per UTC day, newest 7 kept. A minimal `tracing` subscriber, so no
//! extra logging crates. Only this crate's events are written; dependency logs
//! never reach the file. Callers log event names, session ids and timings
//! only: never bodies, tokens or paths.

use std::fmt::{Debug, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};

const KEEP: usize = 7;

/// Installs the file logger. Logging is best-effort: failures are ignored.
pub fn init() {
    let Some(dir) = knock_core::paths::log_dir(&knock_core::paths::process_env) else {
        return;
    };
    if fs::create_dir_all(&dir).is_ok() {
        let _ = tracing::subscriber::set_global_default(FileLog::new(dir));
    }
}

pub struct FileLog {
    dir: PathBuf,
    current: Mutex<Option<(String, File)>>,
}

impl FileLog {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            current: Mutex::new(None),
        }
    }

    fn write_at(&self, now: SystemTime, line: &str) {
        let stamp = utc(now);
        let day = &stamp[..10];
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if current.as_ref().is_none_or(|(d, _)| d != day) {
            let path = self.dir.join(format!("knock.{day}.log"));
            *current = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(|f| (day.to_owned(), f));
            prune(&self.dir, KEEP);
        }
        if let Some((_, f)) = current.as_mut() {
            let _ = writeln!(f, "{stamp} {line}");
        }
    }
}

/// Deletes all but the newest `keep` log files. Names sort by date.
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("knock.") && n.ends_with(".log"))
        .collect();
    logs.sort();
    for name in &logs[..logs.len().saturating_sub(keep)] {
        let _ = fs::remove_file(dir.join(name));
    }
}

impl Subscriber for FileLog {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.is_event()
            && m.target().starts_with(env!("CARGO_CRATE_NAME"))
            && *m.level() <= Level::INFO
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        // Spans are never enabled (events only), so this is never stored.
        span::Id::from_u64(1)
    }
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, e: &Event<'_>) {
        let m = e.metadata();
        let mut line = format!("{} {}:", m.level(), m.target());
        e.record(&mut Line(&mut line));
        self.write_at(SystemTime::now(), &line);
    }
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, f: &Field, v: &dyn Debug) {
        // Field values use Debug, so strings are quoted and escaped: a value
        // can't forge a new log line.
        let _ = if f.name() == "message" {
            write!(self.0, " {v:?}")
        } else {
            write!(self.0, " {}={v:?}", f.name())
        };
    }
}

/// RFC 3339 UTC with milliseconds, e.g. `2026-10-16T14:02:11.120Z`.
pub fn utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (days, s) = ((d.as_secs() / 86_400) as i64, d.as_secs() % 86_400);
    // Days to civil date (H. Hinnant, "chrono-compatible low-level date algorithms").
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        s / 3600,
        s / 60 % 60,
        s % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let leap = UNIX_EPOCH + Duration::from_millis(951_782_400_000 + 5_025_120);
        assert_eq!(utc(leap), "2000-02-29T01:23:45.120Z");
        let t = UNIX_EPOCH + Duration::from_secs(1_792_166_400);
        assert_eq!(utc(t), "2026-10-16T16:00:00.000Z");
    }

    #[test]
    fn rotates_daily_and_keeps_seven() {
        let dir = std::env::temp_dir().join(format!("knock-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = FileLog::new(dir.clone());
        let day0 = UNIX_EPOCH + Duration::from_secs(1_792_166_400);
        for i in 0..9 {
            log.write_at(day0 + Duration::from_secs(86_400 * i), "INFO x: line");
        }
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 7);
        assert_eq!(names[0], "knock.2026-10-18.log");
        assert_eq!(names[6], "knock.2026-10-24.log");
        let last = fs::read_to_string(dir.join(&names[6])).unwrap();
        assert_eq!(last, "2026-10-24T16:00:00.000Z INFO x: line\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
