//! Shared probe value types: capability support and `AdapterProbe`.
//!
//! **Ownership.** Data exchanged between an adapter and detection/planning. No I/O and no
//! KDE knowledge; the KDE implementation fills these values in Phase 6.
//!
//! `FakeAdapter` lives here because it is the cross-cutting test double from TESTING.md
//! §3. It is public (integration tests use it) but is never part of the application path.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use crate::core::capability::Capability;
use crate::core::versions::PlasmaVersion;
use crate::errors::{MorphixError, Result};
use crate::platform::session::SessionType;

use super::{DesktopAdapter, DesktopKind};

/// The support state of one capability on this system.
///
/// Serialized as an adjacently tagged object so the UI never has to infer a state from a
/// missing field: `{"state":"available"}`, `{"state":"degraded","reason":"..."}`, or
/// `{"state":"unavailable"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum CapabilitySupport {
    /// The capability works on this system.
    Available,
    /// The capability works with a limitation the UI must disclose.
    Degraded(DegradedReason),
    /// The capability cannot be used; planning must refuse it.
    Unavailable,
}

impl CapabilitySupport {
    /// Whether the capability can be planned.
    #[must_use]
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available | Self::Degraded(_))
    }
}

/// Why a capability is degraded rather than fully available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DegradedReason {
    /// Icons can be installed and selected, but the change becomes live only after the
    /// shell restarts; Morphix never restarts the shell (ADR-020).
    #[serde(rename = "plasmashell_restart_required")]
    PlasmaShellRestartRequired,
}

/// Per-capability support for this system.
///
/// A missing entry means "not reported", which is treated as [`CapabilitySupport::
/// Unavailable`] by [`CapabilityMatrix::support`]. A probe must not be able to imply
/// support by omission.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityMatrix(BTreeMap<Capability, CapabilitySupport>);

impl CapabilityMatrix {
    /// An empty matrix; every capability reads as `Unavailable`.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// A matrix with every capability in the same state.
    #[must_use]
    pub fn all(support: CapabilitySupport) -> Self {
        Self(
            Capability::ALL
                .into_iter()
                .map(|capability| (capability, support))
                .collect(),
        )
    }

    /// Records one capability's state.
    pub fn set(&mut self, capability: Capability, support: CapabilitySupport) {
        self.0.insert(capability, support);
    }

    /// The recorded state, if the probe reported one.
    #[must_use]
    pub fn get(&self, capability: Capability) -> Option<CapabilitySupport> {
        self.0.get(&capability).copied()
    }

    /// The effective state: an unreported capability is unavailable.
    #[must_use]
    pub fn support(&self, capability: Capability) -> CapabilitySupport {
        self.get(capability)
            .unwrap_or(CapabilitySupport::Unavailable)
    }

    /// Recorded entries in capability order.
    pub fn iter(&self) -> impl Iterator<Item = (&Capability, &CapabilitySupport)> {
        self.0.iter()
    }

    /// Number of recorded entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether all fifteen capabilities have been reported.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.0.len() == Capability::ALL.len()
    }
}

impl FromIterator<(Capability, CapabilitySupport)> for CapabilityMatrix {
    fn from_iter<T: IntoIterator<Item = (Capability, CapabilitySupport)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// The adapter's view of this installation.
///
/// `plasma_version` is `None` when every detection path failed; the status layer reports
/// that as unverified rather than guessing. Capabilities are a closed map, not free text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterProbe {
    /// Detected Plasma version, if any.
    pub plasma_version: Option<PlasmaVersion>,
    /// Current display session.
    pub session_type: SessionType,
    /// Per-capability support.
    pub capabilities: CapabilityMatrix,
}

impl AdapterProbe {
    /// Builds a probe result.
    #[must_use]
    pub fn new(
        plasma_version: Option<PlasmaVersion>,
        session_type: SessionType,
        capabilities: CapabilityMatrix,
    ) -> Self {
        Self {
            plasma_version,
            session_type,
            capabilities,
        }
    }
}

/// One call observed by [`FakeAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterCall {
    /// `DesktopAdapter::probe`.
    Probe,
}

/// A configurable, call-recording `DesktopAdapter` for tests.
///
/// The probe result (success or a typed error) is fixed at construction. `probe` records
/// each call; `kind` is a constant and is not recorded. Later phases extend the recording
/// enum as the adapter gains validate/apply/verify/reverse helpers.
#[derive(Debug)]
pub struct FakeAdapter {
    state: Mutex<FakeState>,
}

#[derive(Debug)]
struct FakeState {
    probe: Result<AdapterProbe>,
    calls: Vec<AdapterCall>,
}

impl FakeAdapter {
    /// A fake that returns `probe` successfully.
    #[must_use]
    pub fn new(probe: AdapterProbe) -> Self {
        Self {
            state: Mutex::new(FakeState {
                probe: Ok(probe),
                calls: Vec::new(),
            }),
        }
    }

    /// A fake whose `probe` returns the supplied error.
    #[must_use]
    pub fn failing(error: MorphixError) -> Self {
        Self {
            state: Mutex::new(FakeState {
                probe: Err(error),
                calls: Vec::new(),
            }),
        }
    }

    /// Recorded calls, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<AdapterCall> {
        self.lock().calls.clone()
    }

    /// Number of recorded calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.lock().calls.len()
    }

    /// Clears the recording without changing the configured probe.
    pub fn reset_calls(&self) {
        self.lock().calls.clear();
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for FakeAdapter {
    fn default() -> Self {
        Self::new(AdapterProbe::default())
    }
}

impl DesktopAdapter for FakeAdapter {
    fn kind(&self) -> DesktopKind {
        DesktopKind::Kde
    }

    fn probe(&self) -> Result<AdapterProbe> {
        let mut state = self.lock();
        state.calls.push(AdapterCall::Probe);
        state.probe.clone()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use serde_json::json;

    use super::{
        AdapterCall, AdapterProbe, CapabilityMatrix, CapabilitySupport, DegradedReason, FakeAdapter,
    };
    use crate::core::capability::Capability;
    use crate::core::versions::PlasmaVersion;
    use crate::desktop::DesktopAdapter;
    use crate::errors::{ErrorCode, MorphixError};
    use crate::platform::session::SessionType;

    fn probe() -> AdapterProbe {
        let mut capabilities = CapabilityMatrix::all(CapabilitySupport::Available);
        capabilities.set(
            Capability::Icons,
            CapabilitySupport::Degraded(DegradedReason::PlasmaShellRestartRequired),
        );

        AdapterProbe::new(
            Some(PlasmaVersion::parse("6.8.0").expect("version")),
            SessionType::Wayland,
            capabilities,
        )
    }

    #[test]
    fn capability_support_serializes_as_a_tagged_state() {
        assert_eq!(
            serde_json::to_value(CapabilitySupport::Available).expect("serializes"),
            json!({ "state": "available" })
        );
        assert_eq!(
            serde_json::to_value(CapabilitySupport::Unavailable).expect("serializes"),
            json!({ "state": "unavailable" })
        );
        assert_eq!(
            serde_json::to_value(CapabilitySupport::Degraded(
                DegradedReason::PlasmaShellRestartRequired
            ))
            .expect("serializes"),
            json!({ "state": "degraded", "reason": "plasmashell_restart_required" })
        );
    }

    #[test]
    fn an_unreported_capability_is_unavailable() {
        let matrix = CapabilityMatrix::new();
        assert!(matrix.is_empty());
        assert_eq!(
            matrix.support(Capability::Wallpaper),
            CapabilitySupport::Unavailable
        );
        assert!(!matrix.support(Capability::Icons).is_available());
    }

    #[test]
    fn a_full_matrix_reports_every_capability() {
        let mut matrix = CapabilityMatrix::all(CapabilitySupport::Available);
        assert!(matrix.is_complete());
        assert_eq!(matrix.len(), 15);
        assert_eq!(matrix.iter().count(), 15);

        matrix.set(Capability::Wallpaper, CapabilitySupport::Unavailable);
        assert_eq!(
            matrix.get(Capability::Wallpaper),
            Some(CapabilitySupport::Unavailable)
        );
        assert!(matrix.is_complete());
    }

    #[test]
    fn adapter_probe_round_trips_through_json() {
        let probe = probe();
        let json = serde_json::to_value(&probe).expect("serializes");

        assert_eq!(json["plasma_version"], "6.8.0");
        assert_eq!(json["session_type"], "wayland");
        assert_eq!(
            json["capabilities"]["appearance.icons"]["state"],
            "degraded"
        );
        assert_eq!(
            json["capabilities"]["appearance.icons"]["reason"],
            "plasmashell_restart_required"
        );

        let back: AdapterProbe = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, probe);
    }

    #[test]
    fn fake_adapter_records_probe_calls_and_repeats_the_result() {
        let probe = probe();
        let fake = FakeAdapter::new(probe.clone());

        assert_eq!(fake.kind(), crate::desktop::DesktopKind::Kde);
        assert_eq!(fake.call_count(), 0, "kind is not recorded");

        assert_eq!(fake.probe().expect("probe succeeds"), probe);
        assert_eq!(fake.probe().expect("probe succeeds"), probe);
        assert_eq!(fake.calls(), [AdapterCall::Probe, AdapterCall::Probe]);

        fake.reset_calls();
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn fake_adapter_returns_a_configured_error_and_still_records_the_call() {
        let error = MorphixError::new(ErrorCode::AdapterUnavailable, "no adapter");
        let fake = FakeAdapter::failing(error.clone());

        let returned = fake.probe().expect_err("probe fails");
        assert_eq!(returned, error);
        assert_eq!(fake.calls(), [AdapterCall::Probe]);
    }

    #[test]
    fn fake_adapter_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FakeAdapter>();
    }
}
