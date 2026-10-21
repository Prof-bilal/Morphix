//! KDE Plasma 6 adapter.
//!
//! **Ownership.** The only module allowed to know KDE session-bus names, config paths, or
//! config formats. This phase provides the session-bus boundary and presence probe; the
//! target map, config read/write, and the full `DesktopAdapter` implementation arrive in
//! Phase 6.
//!
//! **Allowed dependencies:** `errors`, `core`, `fs`, `platform`, and the `zbus` client.
//! **Must never depend on:** `packs`, `planning`, `transactions`, `storage`.

pub mod bus;
pub mod config;
pub mod probe;
pub mod targets;
pub mod wallpaper;

pub use bus::{FakeBus, KDE_BUS_TIMEOUT, KdeBus, KdeService, SessionBus};
pub use probe::{
    DPKG_STATUS_DB, KDEGLOBALS_FILE, KWINRC_FILE, KdeConfigPresence, KdePresence, PACMAN_LOCAL_DB,
    detect_plasma_version, parse_probe_version, probe_config_presence, probe_plasma_version,
    probe_presence,
};
