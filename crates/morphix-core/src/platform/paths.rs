//! XDG base directories and Morphix's own data/state paths.
//!
//! **Ownership.** Resolves the user's XDG directories once, with the test overrides from
//! ARCHITECTURE.md §10. No function in this module creates, reads, or writes a file: it is
//! pure path resolution, so detection can depend on it without touching the user's home.
//!
//! # Rules
//!
//! - `$HOME` is required and must be absolute. Without it there is no safe user-scope
//!   location, so resolution fails with `E_UNSUPPORTED_PLATFORM`.
//! - `XDG_*` values that are relative or empty are ignored, as required by the XDG Base
//!   Directory Specification; the documented default is used instead.
//! - `MORPHIX_DATA_DIR` and `MORPHIX_STATE_DIR` override the whole Morphix root (not the
//!   XDG root). They must be absolute; a relative value is refused rather than resolved
//!   against the process working directory (SECURITY.md: user scope only).
//! - `~` is not expanded. Callers pass real paths.

use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, MorphixError, Result};

/// Directory name below `XDG_DATA_HOME` and `XDG_STATE_HOME`.
pub const MORPHIX_DIR_NAME: &str = "morphix";

/// Default for `XDG_CONFIG_DIRS` (XDG Base Directory Specification).
pub const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";

/// Default for `XDG_DATA_DIRS` (XDG Base Directory Specification).
pub const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// The XDG base directories for the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdgPaths {
    home: PathBuf,
    data_home: PathBuf,
    config_home: PathBuf,
    state_home: PathBuf,
    cache_home: PathBuf,
    config_dirs: Vec<PathBuf>,
    data_dirs: Vec<PathBuf>,
}

impl XdgPaths {
    /// Resolves the paths from the process environment.
    ///
    /// # Errors
    ///
    /// Returns `E_UNSUPPORTED_PLATFORM` when `HOME` is missing or relative.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Resolves the paths from an injected lookup, for deterministic tests.
    ///
    /// The lookup returns `None` for an unset variable. Non-UTF-8 values are treated as
    /// unset by [`XdgPaths::from_env`]; this is deliberate, because every XDG value is
    /// defined as a string of paths.
    ///
    /// # Errors
    ///
    /// Returns `E_UNSUPPORTED_PLATFORM` when `HOME` is missing or relative.
    pub fn from_lookup<F>(lookup: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let home = absolute_value(lookup("HOME")).ok_or_else(|| {
            MorphixError::new(
                ErrorCode::UnsupportedPlatform,
                "HOME is not set to an absolute path; Morphix cannot locate user-scope files",
            )
            .with_context("variable", "HOME")
        })?;

        Ok(Self {
            data_home: xdg_value(&lookup, "XDG_DATA_HOME", home.join(".local/share")),
            config_home: xdg_value(&lookup, "XDG_CONFIG_HOME", home.join(".config")),
            state_home: xdg_value(&lookup, "XDG_STATE_HOME", home.join(".local/state")),
            cache_home: xdg_value(&lookup, "XDG_CACHE_HOME", home.join(".cache")),
            config_dirs: xdg_list(&lookup, "XDG_CONFIG_DIRS", DEFAULT_CONFIG_DIRS),
            data_dirs: xdg_list(&lookup, "XDG_DATA_DIRS", DEFAULT_DATA_DIRS),
            home,
        })
    }

    /// The user's home directory.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `$XDG_DATA_HOME`, default `$HOME/.local/share`.
    #[must_use]
    pub fn data_home(&self) -> &Path {
        &self.data_home
    }

    /// `$XDG_CONFIG_HOME`, default `$HOME/.config`.
    #[must_use]
    pub fn config_home(&self) -> &Path {
        &self.config_home
    }

    /// `$XDG_STATE_HOME`, default `$HOME/.local/state`.
    #[must_use]
    pub fn state_home(&self) -> &Path {
        &self.state_home
    }

    /// `$XDG_CACHE_HOME`, default `$HOME/.cache`.
    #[must_use]
    pub fn cache_home(&self) -> &Path {
        &self.cache_home
    }

    /// `$XDG_CONFIG_DIRS`, default `/etc/xdg`. Relative entries are dropped.
    #[must_use]
    pub fn config_dirs(&self) -> &[PathBuf] {
        &self.config_dirs
    }

    /// `$XDG_DATA_DIRS`, default `/usr/local/share:/usr/share`. Relative entries are
    /// dropped.
    #[must_use]
    pub fn data_dirs(&self) -> &[PathBuf] {
        &self.data_dirs
    }
}

/// Morphix's user-scope data and state roots, plus the documented derived paths.
///
/// All paths here are vocabulary only. Creating them is the caller's job, and only the
/// snapshot/transaction layers may write (ARCHITECTURE.md §10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphixPaths {
    data_dir: PathBuf,
    state_dir: PathBuf,
}

impl MorphixPaths {
    /// Resolves Morphix paths from the process environment, honouring the test overrides.
    ///
    /// # Errors
    ///
    /// Propagates [`XdgPaths::from_env`]'s error, and returns `E_UNSUPPORTED_PLATFORM`
    /// when a Morphix override is relative.
    pub fn from_env() -> Result<Self> {
        let xdg = XdgPaths::from_env()?;
        Self::from_xdg_with(&xdg, |name| std::env::var(name).ok())
    }

    /// Resolves Morphix paths from explicit XDG paths and an injected lookup.
    ///
    /// # Errors
    ///
    /// Returns `E_UNSUPPORTED_PLATFORM` when `MORPHIX_DATA_DIR` or `MORPHIX_STATE_DIR` is
    /// set to a relative path.
    pub fn from_xdg_with<F>(xdg: &XdgPaths, lookup: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let data_dir = resolve_override(
            lookup("MORPHIX_DATA_DIR"),
            xdg.data_home().join(MORPHIX_DIR_NAME),
            "MORPHIX_DATA_DIR",
        )?;
        let state_dir = resolve_override(
            lookup("MORPHIX_STATE_DIR"),
            xdg.state_home().join(MORPHIX_DIR_NAME),
            "MORPHIX_STATE_DIR",
        )?;

        Ok(Self {
            data_dir,
            state_dir,
        })
    }

    /// Root for durable data: database, packs, snapshots, wallpaper assets.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Root for state: logs, locks, recovery reports.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// `~/.local/share/morphix/db/`.
    #[must_use]
    pub fn db_dir(&self) -> PathBuf {
        self.data_dir.join("db")
    }

    /// `~/.local/share/morphix/db/morphix.sqlite3`.
    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.db_dir().join("morphix.sqlite3")
    }

    /// `~/.local/share/morphix/packs/`.
    #[must_use]
    pub fn packs_dir(&self) -> PathBuf {
        self.data_dir.join("packs")
    }

    /// `~/.local/share/morphix/snapshots/`.
    #[must_use]
    pub fn snapshots_dir(&self) -> PathBuf {
        self.data_dir.join("snapshots")
    }

    /// `~/.local/share/morphix/assets/wallpapers/`.
    #[must_use]
    pub fn wallpapers_dir(&self) -> PathBuf {
        self.data_dir.join("assets/wallpapers")
    }

    /// `~/.local/state/morphix/locks/`.
    #[must_use]
    pub fn locks_dir(&self) -> PathBuf {
        self.state_dir.join("locks")
    }

    /// `~/.local/state/morphix/locks/mutation.lock`.
    #[must_use]
    pub fn mutation_lock_path(&self) -> PathBuf {
        self.locks_dir().join("mutation.lock")
    }

    /// `~/.local/state/morphix/logs/`.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    /// `~/.local/state/morphix/logs/morphix.log`.
    #[must_use]
    pub fn log_file_path(&self) -> PathBuf {
        self.logs_dir().join("morphix.log")
    }

    /// `~/.local/state/morphix/recovery/`.
    #[must_use]
    pub fn recovery_dir(&self) -> PathBuf {
        self.state_dir.join("recovery")
    }
}

/// Returns an absolute, non-empty value, or `None`.
fn absolute_value(value: Option<String>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// A single XDG home variable, falling back when unset, empty, or relative.
fn xdg_value<F>(lookup: &F, variable: &str, fallback: PathBuf) -> PathBuf
where
    F: Fn(&str) -> Option<String>,
{
    absolute_value(lookup(variable)).unwrap_or(fallback)
}

/// An XDG search-path variable: colon-separated, absolute entries only.
fn xdg_list<F>(lookup: &F, variable: &str, fallback: &str) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    let configured = lookup(variable)
        .filter(|value| !value.is_empty())
        .map(|value| parse_list(&value))
        .unwrap_or_default();

    if configured.is_empty() {
        parse_list(fallback)
    } else {
        configured
    }
}

fn parse_list(value: &str) -> Vec<PathBuf> {
    value
        .split(':')
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

/// A Morphix override is used verbatim when absolute; empty means unset.
fn resolve_override(value: Option<String>, fallback: PathBuf, variable: &str) -> Result<PathBuf> {
    match value.filter(|value| !value.is_empty()) {
        None => Ok(fallback),
        Some(value) => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                Ok(path)
            } else {
                Err(MorphixError::new(
                    ErrorCode::UnsupportedPlatform,
                    "a Morphix path override must be absolute",
                )
                .with_context("variable", variable)
                .with_context("value", path.display().to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::BTreeMap;

    use super::{MorphixPaths, XdgPaths};
    use crate::errors::ErrorCode;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn xdg(pairs: &[(&str, &str)]) -> XdgPaths {
        XdgPaths::from_lookup(env(pairs)).expect("XDG paths resolve")
    }

    #[test]
    fn defaults_follow_the_xdg_specification() {
        let paths = xdg(&[("HOME", "/home/tester")]);

        assert_eq!(paths.home(), std::path::Path::new("/home/tester"));
        assert_eq!(
            paths.data_home(),
            std::path::Path::new("/home/tester/.local/share")
        );
        assert_eq!(
            paths.config_home(),
            std::path::Path::new("/home/tester/.config")
        );
        assert_eq!(
            paths.state_home(),
            std::path::Path::new("/home/tester/.local/state")
        );
        assert_eq!(
            paths.cache_home(),
            std::path::Path::new("/home/tester/.cache")
        );
        assert_eq!(paths.config_dirs(), [std::path::Path::new("/etc/xdg")]);
        assert_eq!(
            paths.data_dirs(),
            [
                std::path::Path::new("/usr/local/share"),
                std::path::Path::new("/usr/share"),
            ]
        );
    }

    #[test]
    fn absolute_xdg_values_win_over_defaults() {
        let paths = xdg(&[
            ("HOME", "/home/tester"),
            ("XDG_DATA_HOME", "/mnt/data"),
            ("XDG_CONFIG_HOME", "/mnt/config"),
            ("XDG_STATE_HOME", "/mnt/state"),
            ("XDG_CACHE_HOME", "/mnt/cache"),
        ]);

        assert_eq!(paths.data_home(), std::path::Path::new("/mnt/data"));
        assert_eq!(paths.config_home(), std::path::Path::new("/mnt/config"));
        assert_eq!(paths.state_home(), std::path::Path::new("/mnt/state"));
        assert_eq!(paths.cache_home(), std::path::Path::new("/mnt/cache"));
    }

    #[test]
    fn relative_and_empty_xdg_values_are_ignored() {
        let paths = xdg(&[
            ("HOME", "/home/tester"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", "../state"),
            ("XDG_CACHE_HOME", "."),
        ]);

        assert_eq!(
            paths.data_home(),
            std::path::Path::new("/home/tester/.local/share")
        );
        assert_eq!(
            paths.config_home(),
            std::path::Path::new("/home/tester/.config")
        );
        assert_eq!(
            paths.state_home(),
            std::path::Path::new("/home/tester/.local/state")
        );
        assert_eq!(
            paths.cache_home(),
            std::path::Path::new("/home/tester/.cache")
        );
    }

    #[test]
    fn search_paths_drop_relative_entries_and_fall_back_when_empty() {
        let paths = xdg(&[
            ("HOME", "/home/tester"),
            ("XDG_CONFIG_DIRS", "/custom/xdg:relative:/another/xdg"),
            ("XDG_DATA_DIRS", "relative:also-relative"),
        ]);

        assert_eq!(
            paths.config_dirs(),
            [
                std::path::Path::new("/custom/xdg"),
                std::path::Path::new("/another/xdg"),
            ]
        );
        assert_eq!(
            paths.data_dirs(),
            [
                std::path::Path::new("/usr/local/share"),
                std::path::Path::new("/usr/share"),
            ]
        );
    }

    #[test]
    fn missing_or_relative_home_is_refused() {
        for pairs in [vec![], vec![("HOME", "relative")]] {
            let error = XdgPaths::from_lookup(env(&pairs)).expect_err("HOME is required");
            assert_eq!(error.code, ErrorCode::UnsupportedPlatform);
            assert_eq!(error.context[0].key, "variable");
        }
    }

    #[test]
    fn morphix_roots_default_below_the_xdg_homes() {
        let paths = MorphixPaths::from_xdg_with(&xdg(&[("HOME", "/home/tester")]), env(&[]))
            .expect("Morphix paths resolve");

        assert_eq!(
            paths.data_dir(),
            std::path::Path::new("/home/tester/.local/share/morphix")
        );
        assert_eq!(
            paths.state_dir(),
            std::path::Path::new("/home/tester/.local/state/morphix")
        );
    }

    #[test]
    fn morphix_overrides_replace_the_whole_root() {
        let paths = MorphixPaths::from_xdg_with(
            &xdg(&[("HOME", "/home/tester")]),
            env(&[
                ("MORPHIX_DATA_DIR", "/tmp/morphix-data"),
                ("MORPHIX_STATE_DIR", "/tmp/morphix-state"),
            ]),
        )
        .expect("Morphix paths resolve");

        assert_eq!(paths.data_dir(), std::path::Path::new("/tmp/morphix-data"));
        assert_eq!(
            paths.state_dir(),
            std::path::Path::new("/tmp/morphix-state")
        );
        assert_eq!(
            paths.database_path(),
            std::path::Path::new("/tmp/morphix-data/db/morphix.sqlite3")
        );
    }

    #[test]
    fn relative_morphix_overrides_are_refused() {
        let error = MorphixPaths::from_xdg_with(
            &xdg(&[("HOME", "/home/tester")]),
            env(&[("MORPHIX_DATA_DIR", "relative")]),
        )
        .expect_err("relative override must be refused");

        assert_eq!(error.code, ErrorCode::UnsupportedPlatform);
        assert!(error.context.iter().any(|entry| entry.key == "variable"));
    }

    #[test]
    fn derived_paths_match_the_documented_table() {
        let paths = MorphixPaths::from_xdg_with(
            &xdg(&[("HOME", "/home/tester")]),
            env(&[
                ("MORPHIX_DATA_DIR", "/data/morphix"),
                ("MORPHIX_STATE_DIR", "/state/morphix"),
            ]),
        )
        .expect("Morphix paths resolve");

        assert_eq!(paths.db_dir(), std::path::Path::new("/data/morphix/db"));
        assert_eq!(
            paths.database_path(),
            std::path::Path::new("/data/morphix/db/morphix.sqlite3")
        );
        assert_eq!(
            paths.packs_dir(),
            std::path::Path::new("/data/morphix/packs")
        );
        assert_eq!(
            paths.snapshots_dir(),
            std::path::Path::new("/data/morphix/snapshots")
        );
        assert_eq!(
            paths.wallpapers_dir(),
            std::path::Path::new("/data/morphix/assets/wallpapers")
        );
        assert_eq!(
            paths.locks_dir(),
            std::path::Path::new("/state/morphix/locks")
        );
        assert_eq!(
            paths.mutation_lock_path(),
            std::path::Path::new("/state/morphix/locks/mutation.lock")
        );
        assert_eq!(
            paths.logs_dir(),
            std::path::Path::new("/state/morphix/logs")
        );
        assert_eq!(
            paths.log_file_path(),
            std::path::Path::new("/state/morphix/logs/morphix.log")
        );
        assert_eq!(
            paths.recovery_dir(),
            std::path::Path::new("/state/morphix/recovery")
        );
    }
}
