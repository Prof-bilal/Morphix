//! Desktop adapter boundary: the `DesktopAdapter` trait, probe types, and the KDE
//! implementation.
//!
//! **Ownership.** The only module allowed to know KDE paths, config formats, or DBus
//! interfaces. Nothing outside `desktop::kde` may construct a KDE path or call KDE DBus
//! (AGENTS.md §6).
//!
//! **Allowed dependencies:** `errors`, `core`, `fs`, `platform`.
//! **Must never depend on:** `packs`, `planning`, `transactions`, `storage`.
//!
//! **Filled in by:** P1-06 (`mod.rs` trait, `Probe` types, `FakeAdapter`),
//! `probe.rs`, then Phase 6 for `kde/**`.
