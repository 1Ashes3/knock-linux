//! Where Knock keeps its files on each OS (ADR 0037). Pure: the caller
//! passes the environment, so both layouts are tested on any OS.
//!
//! | | Windows | Linux and other unix (XDG) |
//! |---|---|---|
//! | data: `settings.json`, `server.json`, `sessions.json`, `backups/` | `%APPDATA%\Knock` | `$XDG_CONFIG_HOME/knock`, else `~/.config/knock` |
//! | logs | `%LOCALAPPDATA%\Knock\logs` | `$XDG_STATE_HOME/knock/logs`, else `~/.local/state/knock/logs` |
//! | sprite packs | `%APPDATA%\Knock\packs` | `$XDG_DATA_HOME/knock/packs`, else `~/.local/share/knock/packs` |

use std::ffi::OsString;
use std::path::PathBuf;

/// Reads one environment variable.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// The process environment, for [`Env`].
pub fn process_env(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

fn var(env: Env, key: &str) -> Option<PathBuf> {
    env(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// An XDG base directory: `$<xdg>`, else `$HOME/<fallback>`.
fn xdg(env: Env, xdg: &str, fallback: &str) -> Option<PathBuf> {
    var(env, xdg).or_else(|| var(env, "HOME").map(|h| h.join(fallback)))
}

/// The user's home folder.
pub fn home_dir(env: Env) -> Option<PathBuf> {
    home_dir_for(cfg!(windows), env)
}

pub fn home_dir_for(windows: bool, env: Env) -> Option<PathBuf> {
    var(env, if windows { "USERPROFILE" } else { "HOME" })
}

/// Settings, the server's port and token, saved sessions and backups.
pub fn data_dir(env: Env) -> Option<PathBuf> {
    data_dir_for(cfg!(windows), env)
}

pub fn data_dir_for(windows: bool, env: Env) -> Option<PathBuf> {
    match windows {
        true => var(env, "APPDATA").map(|d| d.join("Knock")),
        false => xdg(env, "XDG_CONFIG_HOME", ".config").map(|d| d.join("knock")),
    }
}

/// Daily log files.
pub fn log_dir(env: Env) -> Option<PathBuf> {
    log_dir_for(cfg!(windows), env)
}

pub fn log_dir_for(windows: bool, env: Env) -> Option<PathBuf> {
    match windows {
        true => var(env, "LOCALAPPDATA").map(|d| d.join("Knock").join("logs")),
        false => xdg(env, "XDG_STATE_HOME", ".local/state").map(|d| d.join("knock").join("logs")),
    }
}

/// Installed sprite packs, one folder each.
pub fn packs_dir(env: Env) -> Option<PathBuf> {
    packs_dir_for(cfg!(windows), env)
}

pub fn packs_dir_for(windows: bool, env: Env) -> Option<PathBuf> {
    match windows {
        true => var(env, "APPDATA").map(|d| d.join("Knock").join("packs")),
        false => xdg(env, "XDG_DATA_HOME", ".local/share").map(|d| d.join("knock").join("packs")),
    }
}

/// The user's names, for the adapter's `home` rule (ADR 0020): the login
/// name and the last part of the home folder.
pub fn user_names(env: Env) -> Vec<String> {
    user_names_for(cfg!(windows), env)
}

pub fn user_names_for(windows: bool, env: Env) -> Vec<String> {
    let keys = match windows {
        true => ["USERNAME", "USERPROFILE"],
        false => ["USER", "HOME"],
    };
    keys.iter()
        .filter_map(|k| env(k)?.into_string().ok())
        .filter_map(|v| {
            v.rsplit(['\\', '/'])
                .find(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k| {
            (vars.iter())
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn windows_layout() {
        let e = env(&[
            ("APPDATA", r"C:\Users\A\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\A\AppData\Local"),
            ("USERPROFILE", r"C:\Users\A"),
            ("USERNAME", "A"),
        ]);
        let roaming = PathBuf::from(r"C:\Users\A\AppData\Roaming");
        assert_eq!(data_dir_for(true, &e), Some(roaming.join("Knock")));
        assert_eq!(
            packs_dir_for(true, &e),
            Some(roaming.join("Knock").join("packs"))
        );
        assert_eq!(
            log_dir_for(true, &e),
            Some(
                PathBuf::from(r"C:\Users\A\AppData\Local")
                    .join("Knock")
                    .join("logs")
            )
        );
        assert_eq!(home_dir_for(true, &e), Some(PathBuf::from(r"C:\Users\A")));
        assert_eq!(user_names_for(true, &e), ["A", "A"]);
    }

    #[test]
    fn xdg_layout_with_and_without_the_variables() {
        let bare = env(&[("HOME", "/home/a"), ("USER", "a")]);
        assert_eq!(
            data_dir_for(false, &bare),
            Some(PathBuf::from("/home/a/.config/knock"))
        );
        assert_eq!(
            log_dir_for(false, &bare),
            Some(PathBuf::from("/home/a/.local/state/knock/logs"))
        );
        assert_eq!(
            packs_dir_for(false, &bare),
            Some(PathBuf::from("/home/a/.local/share/knock/packs"))
        );
        assert_eq!(user_names_for(false, &bare), ["a", "a"]);
        let xdg = env(&[
            ("HOME", "/home/a"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_STATE_HOME", "/s"),
            ("XDG_DATA_HOME", "/d"),
        ]);
        assert_eq!(data_dir_for(false, &xdg), Some(PathBuf::from("/c/knock")));
        assert_eq!(
            log_dir_for(false, &xdg),
            Some(PathBuf::from("/s/knock/logs"))
        );
        assert_eq!(
            packs_dir_for(false, &xdg),
            Some(PathBuf::from("/d/knock/packs"))
        );
        // An empty variable counts as unset.
        let empty = env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "")]);
        assert_eq!(
            data_dir_for(false, &empty),
            Some(PathBuf::from("/home/a/.config/knock"))
        );
        assert_eq!(data_dir_for(false, &env(&[])), None);
    }
}
