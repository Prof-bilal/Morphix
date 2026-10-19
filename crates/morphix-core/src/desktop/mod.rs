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
//! # Trait scope
//!
//! This phase defines the stable core of the boundary: identifying the desktop and
//! probing it. ARCHITECTURE.md §9.1 also specifies `resolve_target`, `read_state`,
//! `validate_value`, and `notify`; those methods depend on `ResolvedTarget`,
//! `ResourceRef`, `ConfigValue`, and `ChangeNotification`, which arrive with the planner
//! and KDE adapter in Phase 6. They are deliberately not stubbed with placeholder types.
//!
//! Apply/verify/reverse are *not* trait methods by design: operation executors in
//! `transactions` own those semantics and call adapter helpers.

use serde::{Deserialize, Serialize};

use crate::errors::Result;

pub mod kde;
pub mod probe;

pub use probe::{
    AdapterCall, AdapterProbe, CapabilityMatrix, CapabilitySupport, DegradedReason, FakeAdapter,
};

/// The desktop an adapter implements.
///
/// V0.1 supports KDE Plasma 6 only (ADR-004); a second variant requires a new ADR, not a
/// configuration flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopKind {
    /// KDE Plasma 6.
    #[default]
    Kde,
}

/// The boundary between the domain engine and a desktop environment.
///
/// Implementations must be safe to share across threads; detection and planning run on
/// blocking worker threads.
pub trait DesktopAdapter: Send + Sync {
    /// Which desktop this adapter targets.
    fn kind(&self) -> DesktopKind;

    /// Reports the detected version, session, and per-capability support.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the adapter cannot probe at all; a capability that is
    /// merely unsupported is reported as [`CapabilitySupport::Unavailable`], not an error.
    fn probe(&self) -> Result<AdapterProbe>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{AdapterProbe, DesktopAdapter, DesktopKind, FakeAdapter};

    #[test]
    fn kde_serializes_to_the_closed_name() {
        assert_eq!(
            serde_json::to_string(&DesktopKind::Kde).expect("serializes"),
            "\"kde\""
        );
        assert_eq!(
            serde_json::from_str::<DesktopKind>("\"kde\"").expect("deserializes"),
            DesktopKind::Kde
        );
        assert!(serde_json::from_str::<DesktopKind>("\"gnome\"").is_err());
    }

    #[test]
    fn the_trait_is_object_safe_and_usable_through_a_reference() {
        let fake = FakeAdapter::new(AdapterProbe::default());
        let adapter: &dyn DesktopAdapter = &fake;

        assert_eq!(adapter.kind(), DesktopKind::Kde);
        assert!(adapter.probe().is_ok());
    }
}
