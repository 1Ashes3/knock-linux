//! Settings (SPEC.md §8.6): `%APPDATA%\Knock\settings.json`, in exactly the
//! §8.6 shape. Defaults on first run; an invalid file is kept aside and
//! replaced by defaults. Changes come as a JSON merge patch (RFC 7386) and are
//! validated before they are saved. Also the atomic write the app's other
//! files use.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE: &str = "settings.json";
/// Upper bound for `stallMinutes` and `escalationMinutes`: one day.
const MAX_MINUTES: u32 = 24 * 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub monitor: String,
    pub mode: Mode,
    pub size: Size,
    pub sounds: bool,
    pub stall_minutes: u32,
    pub escalation_minutes: u32,
    pub notify_on_stall: bool,
    pub pause_notifications: bool,
    pub quiet_hours: QuietHours,
    /// `null` until setup asks (§8.6).
    pub launch_at_sign_in: Option<bool>,
    /// `knock_core::project_key` → animal id.
    pub animal_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Animals,
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    /// Strip height and slot width in logical px (§9.2).
    pub fn height(self) -> f64 {
        match self {
            Self::Small => 48.0,
            Self::Medium => 64.0,
            Self::Large => 80.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    pub enabled: bool,
    /// `HH:MM`, local time.
    pub start: String,
    pub end: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 1,
            monitor: "primary".into(),
            mode: Mode::Animals,
            size: Size::Medium,
            sounds: false,
            stall_minutes: 10,
            escalation_minutes: 5,
            notify_on_stall: false,
            pause_notifications: false,
            quiet_hours: QuietHours {
                enabled: false,
                start: "22:00".into(),
                end: "08:00".into(),
            },
            launch_at_sign_in: None,
            animal_overrides: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// What the engine needs (§8.4). The v1 animals come in Phase 3.
    pub fn engine(&self) -> knock_core::Settings {
        knock_core::Settings {
            stall_ms: u64::from(self.stall_minutes) * 60_000,
            escalation_ms: u64::from(self.escalation_minutes) * 60_000,
            notify_on_stall: self.notify_on_stall,
            animals: vec!["fox".into()],
            animal_overrides: self.animal_overrides.clone().into_iter().collect(),
        }
    }

    /// §8.4 suppression: `pauseNotifications`, or quiet hours at
    /// `local_minute` (minutes since local midnight). A range whose end is
    /// before its start runs across midnight; start equal to end is empty.
    pub fn suppressed(&self, local_minute: u32) -> bool {
        let q = &self.quiet_hours;
        let quiet = match (minutes(&q.start), minutes(&q.end)) {
            (Some(s), Some(e)) if s <= e => (s..e).contains(&local_minute),
            (Some(s), Some(e)) => local_minute >= s || local_minute < e,
            _ => false,
        };
        self.pause_notifications || (q.enabled && quiet)
    }

    fn validate(&self) -> Result<(), String> {
        let id = |s: &str, max: usize| {
            (1..=max).contains(&s.len())
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        };
        let hex16 = |k: &str| {
            k.len() == 16
                && k.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if self.version != 1 {
            return Err("version must be 1".into());
        }
        if !id(&self.monitor, 128) {
            return Err("monitor must be \"primary\" or a monitor id".into());
        }
        for (name, v) in [
            ("stallMinutes", self.stall_minutes),
            ("escalationMinutes", self.escalation_minutes),
        ] {
            if !(1..=MAX_MINUTES).contains(&v) {
                return Err(format!("{name} must be from 1 to {MAX_MINUTES}"));
            }
        }
        if minutes(&self.quiet_hours.start).is_none() || minutes(&self.quiet_hours.end).is_none() {
            return Err("quiet hours must be times like 22:00".into());
        }
        if !self
            .animal_overrides
            .iter()
            .all(|(k, v)| hex16(k) && id(v, 64))
        {
            return Err("animalOverrides must map project keys to animal ids".into());
        }
        Ok(())
    }
}

/// `HH:MM` (24-hour) → minutes since midnight.
fn minutes(hhmm: &str) -> Option<u32> {
    let b = hhmm.as_bytes();
    let digit = |i: usize| b[i].is_ascii_digit().then(|| u32::from(b[i] - b'0'));
    if b.len() != 5 || b[2] != b':' {
        return None;
    }
    let (h, m) = (digit(0)? * 10 + digit(1)?, digit(3)? * 10 + digit(4)?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn parse(bytes: &[u8]) -> Result<Settings, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let s: Settings = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    s.validate()?;
    Ok(s)
}

/// `%APPDATA%\Knock`, where `server.json`, `settings.json` and
/// `sessions.json` live.
pub fn data_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("APPDATA").ok_or_else(|| io::Error::other("APPDATA not set"))?;
    Ok(PathBuf::from(base).join("Knock"))
}

/// Reads `settings.json` from `dir`. No file: defaults, saved. An invalid
/// file is renamed to `settings.invalid-<ms>.json` and replaced by defaults.
/// Never fails: a read or write error leaves the defaults in memory.
pub fn load(dir: &Path, now_ms: u64) -> Settings {
    let path = dir.join(FILE);
    let found = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "settings file unreadable; using defaults");
            return Settings::default();
        }
    };
    if let Some(bytes) = found {
        match parse(&bytes) {
            Ok(s) => return s,
            Err(_) => {
                // The reason can quote the file; it is not logged.
                let aside = dir.join(format!("settings.invalid-{now_ms}.json"));
                let kept = std::fs::rename(&path, aside).is_ok();
                tracing::warn!(kept, "settings file invalid; kept aside, using defaults");
            }
        }
    }
    let s = Settings::default();
    if let Err(e) = save(dir, &s) {
        tracing::warn!(kind = ?e.kind(), "default settings not saved");
    }
    s
}

pub fn save(dir: &Path, s: &Settings) -> io::Result<()> {
    write_atomic(&dir.join(FILE), &serde_json::to_vec_pretty(s)?)
}

/// `current` with `patch` (RFC 7386 merge patch) applied, if the result is
/// still valid §8.6 settings.
pub fn merged(current: &Settings, patch: &Value) -> Result<Settings, String> {
    if !patch.is_object() {
        return Err("settings change must be an object".into());
    }
    let mut v = serde_json::to_value(current).map_err(|e| e.to_string())?;
    merge(&mut v, patch);
    let s: Settings = serde_json::from_value(v).map_err(|e| e.to_string())?;
    s.validate()?;
    Ok(s)
}

fn merge(target: &mut Value, patch: &Value) {
    let Value::Object(p) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Default::default());
    }
    let Value::Object(t) = target else { return };
    for (k, v) in p {
        if v.is_null() {
            t.remove(k);
        } else {
            merge(t.entry(k.clone()).or_insert(Value::Null), v);
        }
    }
}

/// Writes `bytes` to a temporary file next to `path`, flushes it to disk,
/// then replaces `path` with it, so readers see the old or the new file only.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_if(path, bytes, || true).map(|_| ())
}

/// [`write_atomic`], but `still()` is asked just before the replace; `false`
/// replaces nothing and returns `Ok(false)`. The temporary file is deleted
/// whenever `path` isn't replaced, since it can hold a token.
pub fn write_atomic_if(
    path: &Path,
    bytes: &[u8],
    still: impl FnOnce() -> bool,
) -> io::Result<bool> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let done = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if !still() {
            return Ok(false);
        }
        std::fs::rename(&tmp, path).map(|_| true)
    })();
    if !matches!(done, Ok(true)) {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("knock-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The §8.6 example, verbatim.
    const SPEC: &str = r#"{
      "version": 1,
      "monitor": "primary",
      "mode": "animals",
      "size": "medium",
      "sounds": false,
      "stallMinutes": 10,
      "escalationMinutes": 5,
      "notifyOnStall": false,
      "pauseNotifications": false,
      "quietHours": { "enabled": false, "start": "22:00", "end": "08:00" },
      "launchAtSignIn": null,
      "animalOverrides": {}
    }"#;

    #[test]
    fn first_run_writes_the_exact_spec_defaults() {
        let dir = temp_dir("first-run");
        assert_eq!(load(&dir, 1), Settings::default());
        let saved: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(saved, serde_json::from_str::<Value>(SPEC).unwrap());
        assert!(!dir.join("settings.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_valid_file_loads_as_saved_even_with_a_bom() {
        let dir = temp_dir("valid");
        let mut s = Settings {
            pause_notifications: true,
            size: Size::Large,
            launch_at_sign_in: Some(true),
            ..Settings::default()
        };
        s.animal_overrides
            .insert("0123456789abcdef".into(), "fox".into());
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend(serde_json::to_vec(&s).unwrap());
        std::fs::write(dir.join(FILE), bytes).unwrap();
        assert_eq!(load(&dir, 1), s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invalid_file_is_kept_aside_and_replaced_by_defaults() {
        for (i, bad) in [
            "not json".to_string(),
            SPEC.replace("\"version\": 1", "\"version\": 2"),
            SPEC.replace("\"stallMinutes\": 10", "\"stallMinutes\": 0"),
            SPEC.replace("\"22:00\"", "\"25:00\""),
            SPEC.replace("\"medium\"", "\"huge\""),
            SPEC.replace("\"sounds\": false,", ""),
            SPEC.replace("\"sounds\": false,", "\"sounds\": false, \"extra\": 1,"),
        ]
        .iter()
        .enumerate()
        {
            let dir = temp_dir(&format!("invalid-{i}"));
            std::fs::write(dir.join(FILE), bad).unwrap();
            assert_eq!(load(&dir, 42), Settings::default(), "{bad}");
            let aside = std::fs::read_to_string(dir.join("settings.invalid-42.json")).unwrap();
            assert_eq!(&aside, bad);
            let now = std::fs::read(dir.join(FILE)).unwrap();
            assert_eq!(parse(&now).unwrap(), Settings::default());
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn a_patch_merges_and_validates() {
        let d = Settings::default();
        let s = merged(
            &d,
            &json!({ "pauseNotifications": true, "quietHours": { "enabled": true } }),
        )
        .unwrap();
        assert!(s.pause_notifications && s.quiet_hours.enabled);
        assert_eq!(
            (s.quiet_hours.start.as_str(), s.quiet_hours.end.as_str()),
            ("22:00", "08:00")
        );
        assert_eq!(s.mode, Mode::Animals);

        let s = merged(&d, &json!({ "launchAtSignIn": true })).unwrap();
        assert_eq!(s.launch_at_sign_in, Some(true));
        assert_eq!(
            merged(&s, &json!({ "launchAtSignIn": null }))
                .unwrap()
                .launch_at_sign_in,
            None
        );

        let key = "0123456789abcdef";
        let s = merged(&d, &json!({ "animalOverrides": { key: "fox" } })).unwrap();
        assert_eq!(s.animal_overrides[key], "fox");
        let s = merged(&s, &json!({ "animalOverrides": { key: null } })).unwrap();
        assert!(s.animal_overrides.is_empty());

        assert_eq!(merged(&d, &json!({})).unwrap(), d);
        for bad in [
            json!(true),
            json!([1]),
            json!({ "version": 2 }),
            json!({ "nope": 1 }),
            json!({ "sounds": "yes" }),
            json!({ "sounds": null }),
            json!({ "stallMinutes": 0 }),
            json!({ "escalationMinutes": 1441 }),
            json!({ "quietHours": { "start": "8:00" } }),
            json!({ "quietHours": { "end": "08:60" } }),
            json!({ "quietHours": { "other": 1 } }),
            json!({ "mode": "fancy" }),
            json!({ "monitor": "" }),
            json!({ "animalOverrides": { "C:\\work": "fox" } }),
            json!({ "animalOverrides": { key: "" } }),
        ] {
            assert!(merged(&d, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn engine_settings_in_ms_with_the_fox() {
        let mut s = Settings {
            notify_on_stall: true,
            ..Settings::default()
        };
        s.animal_overrides
            .insert("0123456789abcdef".into(), "owl".into());
        let e = s.engine();
        assert_eq!((e.stall_ms, e.escalation_ms), (600_000, 300_000));
        assert!(e.notify_on_stall);
        assert_eq!(e.animals, ["fox"]);
        assert_eq!(e.animal_overrides["0123456789abcdef"], "owl");
    }

    #[test]
    fn suppressed_by_pause_or_quiet_hours_across_midnight() {
        let at = |h: u32, m: u32| h * 60 + m;
        let mut s = Settings::default();
        assert!(!s.suppressed(at(23, 0)), "quiet hours off by default");
        s.quiet_hours.enabled = true; // 22:00-08:00
        for (t, want) in [
            (at(21, 59), false),
            (at(22, 0), true),
            (at(23, 59), true),
            (at(0, 0), true),
            (at(3, 0), true),
            (at(7, 59), true),
            (at(8, 0), false),
            (at(12, 0), false),
        ] {
            assert_eq!(s.suppressed(t), want, "{t}");
        }
        s.quiet_hours.start = "09:00".into();
        s.quiet_hours.end = "17:00".into();
        assert!(!s.suppressed(at(8, 59)) && s.suppressed(at(9, 0)) && !s.suppressed(at(17, 0)));
        s.quiet_hours.end = "09:00".into();
        assert!(!s.suppressed(at(9, 0)), "start == end is an empty range");
        s.pause_notifications = true;
        assert!(s.suppressed(at(12, 0)));
        s.quiet_hours.enabled = false;
        assert!(s.suppressed(at(12, 0)));
    }

    #[test]
    fn a_failed_or_refused_replace_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let tmp = dir.join("f.json.tmp");
        // A folder where the file goes: the rename fails.
        let blocked = dir.join("f.json");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"token").is_err());
        assert!(!tmp.exists(), "the temp file holds the token");
        std::fs::remove_dir(&blocked).unwrap();
        // Refused just before the replace: nothing written, no temp file.
        std::fs::write(&blocked, b"old").unwrap();
        assert!(!write_atomic_if(&blocked, b"new", || false).unwrap());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"old");
        assert!(!tmp.exists());
        assert!(write_atomic_if(&blocked, b"new", || true).unwrap());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"new");
        assert!(!tmp.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn minutes_parses_hh_mm_only() {
        assert_eq!(minutes("00:00"), Some(0));
        assert_eq!(minutes("23:59"), Some(1439));
        for bad in [
            "24:00",
            "12:60",
            "1:00",
            "12-00",
            "12:000",
            "",
            "ab:cd",
            "１２:00",
        ] {
            assert_eq!(minutes(bad), None, "{bad}");
        }
    }
}
