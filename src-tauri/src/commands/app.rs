//! App-level IPC commands available before the domain services are wired.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Stable runtime information needed by Settings and Diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub build: String,
    pub log_path: String,
    pub data_dir: String,
    pub state_dir: String,
}

/// Return the app version/build and the standard Morphix XDG storage locations.
#[tauri::command]
pub fn get_app_info() -> Result<AppInfo, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_dir = morphix_dir(
        std::env::var_os("MORPHIX_DATA_DIR").as_deref(),
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        home.as_deref(),
        ".local/share",
    )?;
    let state_dir = morphix_dir(
        std::env::var_os("MORPHIX_STATE_DIR").as_deref(),
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        home.as_deref(),
        ".local/state",
    )?;
    let log_path = state_dir.join("logs/morphix.log");

    Ok(AppInfo {
        version: morphix_core::VERSION.to_owned(),
        build: option_env!("MORPHIX_BUILD_ID")
            .unwrap_or("development")
            .to_owned(),
        log_path: display_path(&log_path),
        data_dir: display_path(&data_dir),
        state_dir: display_path(&state_dir),
    })
}

fn morphix_dir(
    override_dir: Option<&std::ffi::OsStr>,
    xdg_home: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
    fallback: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = override_dir.filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let base = if let Some(xdg_home) = xdg_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        xdg_home
    } else if let Some(home) = home {
        home.join(fallback)
    } else {
        return Err("HOME is not set and no absolute XDG directory is available".to_owned());
    };

    Ok(base.join("morphix"))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::morphix_dir;

    #[test]
    fn defaults_to_documented_xdg_locations_under_home() {
        let home = Path::new("/home/tester");
        assert_eq!(
            morphix_dir(None, None, Some(home), ".local/share"),
            Ok(Path::new("/home/tester/.local/share/morphix").to_path_buf())
        );
        assert_eq!(
            morphix_dir(None, None, Some(home), ".local/state"),
            Ok(Path::new("/home/tester/.local/state/morphix").to_path_buf())
        );
    }

    #[test]
    fn absolute_xdg_and_morphix_overrides_take_precedence() {
        let home = Path::new("/home/tester");
        assert_eq!(
            morphix_dir(
                None,
                Some(OsStr::new("/mnt/data")),
                Some(home),
                ".local/share"
            ),
            Ok(Path::new("/mnt/data/morphix").to_path_buf())
        );
        assert_eq!(
            morphix_dir(
                Some(OsStr::new("/tmp/morphix-test")),
                Some(OsStr::new("/mnt/data")),
                Some(home),
                ".local/share",
            ),
            Ok(Path::new("/tmp/morphix-test").to_path_buf())
        );
    }

    #[test]
    fn relative_xdg_values_are_ignored_and_home_is_required_for_fallback() {
        assert_eq!(
            morphix_dir(
                None,
                Some(OsStr::new("relative")),
                Some(Path::new("/home/tester")),
                ".local/share",
            ),
            Ok(Path::new("/home/tester/.local/share/morphix").to_path_buf())
        );
        assert!(morphix_dir(None, None, None, ".local/share").is_err());
    }
}
