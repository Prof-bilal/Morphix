//! Wallpaper descriptor reader for pre-state capture (Phase 4 scope).
//!
//! **Ownership.** The only place that knows where Plasma records the wallpaper: the
//! `org.kde.image` slideshow block inside `plasma-org.kde.plasma.desktop-appletsrc`.
//!
//! **Limitation, stated plainly.** This helper reads the *file*, not the live
//! containment layout: it returns the first image-wallpaper block in the file. Which
//! containment backs which desktop is live session state that needs the Phase 6
//! adapter read-back channel. Until then, capture records this descriptor plus the
//! whole appletsrc file, so restore can put both back; multi-desktop precision
//! arrives with Phase 6 and is refused as `E_ADAPTER_UNAVAILABLE` at plan time
//! until then (ADR-024).
//!
//! **Assumption (Plasma 6, all 6.x).** The block shape is
//! `[Containments][<id>][Wallpaper][org.kde.image][General]` with an `Image=`
//! entry holding a `file://` URI. The containment id is numeric but never assumed
//! to be small or dense.

use crate::errors::{ErrorCode, MorphixError, Result};

/// A captured wallpaper descriptor (mirrors `PreState::Wallpaper`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperDesc {
    /// Wallpaper plugin id (`org.kde.image` from this reader).
    pub plugin: String,
    /// Image URI as recorded (`file:///...`).
    pub image: String,
}

/// Reads the first `org.kde.image` wallpaper block from appletsrc bytes.
///
/// Returns `Ok(None)` when the file has no image-wallpaper block — no descriptor is
/// a real pre-state, not an error.
///
/// # Errors
///
/// Returns `E_SNAPSHOT_UNREADABLE` when the bytes are not valid UTF-8.
pub fn read_image_wallpaper(appletsrc: &[u8]) -> Result<Option<WallpaperDesc>> {
    let text = std::str::from_utf8(appletsrc).map_err(|error| {
        MorphixError::new(
            ErrorCode::SnapshotUnreadable,
            "the appletsrc file is not valid UTF-8",
        )
        .with_context("reason", error.to_string())
    })?;
    let mut current: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            current = Some(line[1..line.len() - 1].to_owned());
            continue;
        }
        let Some(group) = current.as_deref() else {
            continue;
        };
        if !is_image_wallpaper_general(group) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "Image"
        {
            let image = value.trim().to_owned();
            if !image.is_empty() {
                return Ok(Some(WallpaperDesc {
                    plugin: "org.kde.image".to_owned(),
                    image,
                }));
            }
        }
    }
    Ok(None)
}

/// Matches `[Containments][<id>][Wallpaper][org.kde.image][General]`, tolerating any
/// containment id and a trailing `[$i]` marker.
fn is_image_wallpaper_general(group: &str) -> bool {
    let mut segments: Vec<&str> = group
        .split("][")
        .map(|segment| segment.trim_matches(['[', ']']).trim())
        .collect();
    if segments.last() == Some(&"$i") {
        segments.pop();
    }
    matches!(
        segments.as_slice(),
        ["Containments", _, "Wallpaper", "org.kde.image", "General"]
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{is_image_wallpaper_general, read_image_wallpaper};

    const APPLETSRC: &[u8] = b"[Containments][1]\nplugin=org.kde.panel\n\n[Containments][2][Wallpaper][org.kde.image][General]\nImage=file:///home/tester/Pictures/wall.jpg\n\n[Containments][3][Wallpaper][org.kde.slideshow][General]\nImage=file:///home/tester/Pictures/other.jpg\n";

    #[test]
    fn reads_the_first_image_wallpaper_block() {
        assert_eq!(
            read_image_wallpaper(APPLETSRC).expect("read"),
            Some(super::WallpaperDesc {
                plugin: "org.kde.image".to_owned(),
                image: "file:///home/tester/Pictures/wall.jpg".to_owned(),
            })
        );
    }

    #[test]
    fn missing_blocks_are_absent() {
        assert_eq!(
            read_image_wallpaper(b"[Containments][1]\nplugin=org.kde.panel\n").expect("read"),
            None
        );
        assert_eq!(read_image_wallpaper(b"").expect("read"), None);
    }

    #[test]
    fn group_matching_tolerates_ids_and_markers() {
        assert!(is_image_wallpaper_general(
            "Containments][27][Wallpaper][org.kde.image][General"
        ));
        assert!(is_image_wallpaper_general(
            "Containments][2][Wallpaper][org.kde.image][General][$i"
        ));
        assert!(!is_image_wallpaper_general(
            "Containments][2][Wallpaper][org.kde.slideshow][General"
        ));
        assert!(!is_image_wallpaper_general("Containments][2][General"));
        assert!(!is_image_wallpaper_general(
            "Containments][2][Wallpaper][org.kde.image][General][Extra"
        ));
    }
}
