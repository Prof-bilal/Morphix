//! Tauri build script: validates `tauri.conf.json` and generates the context code.
//!
//! Keep this file free of logic. Anything conditional about the build belongs in the
//! configuration, where it is reviewable.

fn main() {
    tauri_build::build();
}
