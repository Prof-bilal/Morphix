//! The KDE session-bus boundary.
//!
//! **Ownership.** The only place that opens a session bus or decides which KDE service
//! names may be queried. It exposes presence only, for a closed set of services; there is
//! no generic "call any method" escape hatch (SECURITY.md §9).
//!
//! `SessionBus` is the production implementation. `FakeBus` is the test double that keeps
//! detection and transaction tests independent of a real session bus (ARCHITECTURE.md
//! §16).
//!
//! # Why only `NameHasOwner`
//!
//! Presence is the one question detection needs: is the shell/compositor/shortcut daemon
//! running? Answering it through `org.freedesktop.DBus.NameHasOwner` needs no KDE-specific
//! method and cannot mutate state. Version and capability probes that need richer calls
//! are added with their own typed methods in the adapter phase; none of them may accept a
//! method name, service name, or argument from a pack.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbus::names::BusName;

use crate::errors::{ErrorCode, MorphixError, Result};

/// Every DBus call has a 2 s timeout (SECURITY.md §9).
pub const KDE_BUS_TIMEOUT: Duration = Duration::from_secs(2);

/// A KDE service whose presence detection may ask about.
///
/// The closed enum is the allowlist: a caller cannot pass an arbitrary bus name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KdeService {
    /// The Plasma shell; its absence means no Plasma session.
    PlasmaShell,
    /// The KWin compositor.
    KWin,
    /// The global shortcut daemon.
    KGlobalAccel,
}

impl KdeService {
    /// The well-known session-bus name.
    #[must_use]
    pub fn bus_name(self) -> &'static str {
        match self {
            Self::PlasmaShell => "org.kde.plasmashell",
            Self::KWin => "org.kde.KWin",
            Self::KGlobalAccel => "org.kde.kglobalaccel",
        }
    }

    /// Stable label for logs and error context.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::PlasmaShell => "plasmashell",
            Self::KWin => "kwin",
            Self::KGlobalAccel => "kglobalaccel",
        }
    }

    /// Every service detection cares about.
    #[must_use]
    pub fn all() -> &'static [KdeService] {
        &[Self::PlasmaShell, Self::KWin, Self::KGlobalAccel]
    }
}

/// Presence queries against the session bus.
///
/// Implementations must be thread-safe: detection may run on a blocking worker.
pub trait KdeBus: Send + Sync {
    /// Whether `service` currently has an owner on the session bus.
    ///
    /// `false` is a normal answer (the service is not running). An error means the bus
    /// itself could not answer and is reported as `E_ADAPTER_UNAVAILABLE`; callers must
    /// not interpret an error as "not running".
    fn has_owner(&self, service: KdeService) -> Result<bool>;
}

/// The real session bus, over zbus's blocking API.
#[derive(Debug)]
pub struct SessionBus {
    connection: zbus::blocking::Connection,
}

impl SessionBus {
    /// Connects to the user session bus with the documented timeout.
    ///
    /// # Errors
    ///
    /// Returns `E_ADAPTER_UNAVAILABLE` with `reason = session_bus_unavailable` when there
    /// is no reachable session bus. Callers map that to the `NO_SESSION_BUS` support
    /// reason; it is never treated as a successful probe.
    pub fn connect() -> Result<Self> {
        let connection = zbus::blocking::connection::Builder::session()
            .map_err(|error| bus_unavailable(None, "session_bus_unavailable", &error))?
            .method_timeout(KDE_BUS_TIMEOUT)
            .build()
            .map_err(|error| bus_unavailable(None, "session_bus_unavailable", &error))?;

        Ok(Self { connection })
    }
}

impl KdeBus for SessionBus {
    fn has_owner(&self, service: KdeService) -> Result<bool> {
        let proxy = zbus::blocking::fdo::DBusProxy::new(&self.connection)
            .map_err(|error| bus_unavailable(Some(service), "presence_proxy_failed", &error))?;

        let name = BusName::try_from(service.bus_name())
            .map_err(|_| service_error(service, "invalid_service_name"))?;

        proxy
            .name_has_owner(name)
            .map_err(|error| bus_unavailable(Some(service), "presence_call_failed", &error))
    }
}

/// An in-memory bus for tests.
#[derive(Debug, Default)]
pub struct FakeBus {
    state: Mutex<FakeBusState>,
}

#[derive(Debug, Default)]
struct FakeBusState {
    owners: HashMap<KdeService, bool>,
    failure: Option<MorphixError>,
    calls: Vec<KdeService>,
}

impl FakeBus {
    /// A bus on which no service is running.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks one service as running.
    #[must_use]
    pub fn with_owner(self, service: KdeService) -> Self {
        self.lock().owners.insert(service, true);
        self
    }

    /// Marks one service as not running, cancelling a previous `with_owner`.
    #[must_use]
    pub fn without_owner(self, service: KdeService) -> Self {
        self.lock().owners.insert(service, false);
        self
    }

    /// A bus on which all three services are running.
    #[must_use]
    pub fn all_present() -> Self {
        Self::new()
            .with_owner(KdeService::PlasmaShell)
            .with_owner(KdeService::KWin)
            .with_owner(KdeService::KGlobalAccel)
    }

    /// A bus whose calls return the supplied error.
    #[must_use]
    pub fn failing(error: MorphixError) -> Self {
        let bus = Self::default();
        bus.lock().failure = Some(error);
        bus
    }

    /// Recorded calls, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<KdeService> {
        self.lock().calls.clone()
    }

    /// Clears the recorded calls.
    pub fn reset_calls(&self) {
        self.lock().calls.clear();
    }

    fn lock(&self) -> MutexGuard<'_, FakeBusState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl KdeBus for FakeBus {
    fn has_owner(&self, service: KdeService) -> Result<bool> {
        let mut state = self.lock();
        state.calls.push(service);

        if let Some(error) = &state.failure {
            return Err(error.clone());
        }

        Ok(state.owners.get(&service).copied().unwrap_or(false))
    }
}

fn bus_unavailable(
    service: Option<KdeService>,
    reason: &'static str,
    error: impl std::fmt::Display,
) -> MorphixError {
    let mut error = MorphixError::new(
        ErrorCode::AdapterUnavailable,
        "the KDE session bus could not answer a presence query",
    )
    .with_context("reason", reason)
    .with_context("zbus_error", error.to_string());

    if let Some(service) = service {
        error = error.with_context("service", service.bus_name());
    }
    error
}

fn service_error(service: KdeService, reason: &'static str) -> MorphixError {
    MorphixError::new(
        ErrorCode::AdapterUnavailable,
        "the KDE service name could not be queried",
    )
    .with_context("service", service.bus_name())
    .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{FakeBus, KDE_BUS_TIMEOUT, KdeBus, KdeService};
    use crate::errors::{ErrorCode, MorphixError};

    #[test]
    fn service_names_are_the_documented_kde_names() {
        assert_eq!(KdeService::PlasmaShell.bus_name(), "org.kde.plasmashell");
        assert_eq!(KdeService::KWin.bus_name(), "org.kde.KWin");
        assert_eq!(KdeService::KGlobalAccel.bus_name(), "org.kde.kglobalaccel");
        assert_eq!(KdeService::all().len(), 3);
        assert_eq!(KdeService::PlasmaShell.label(), "plasmashell");
    }

    #[test]
    fn the_timeout_matches_the_dbus_policy() {
        assert_eq!(KDE_BUS_TIMEOUT, std::time::Duration::from_secs(2));
    }

    #[test]
    fn a_fake_bus_reports_only_configured_owners_and_records_calls() {
        let bus = FakeBus::new().with_owner(KdeService::PlasmaShell);

        assert!(bus.has_owner(KdeService::PlasmaShell).expect("query"));
        assert!(!bus.has_owner(KdeService::KWin).expect("query"));
        assert!(!bus.has_owner(KdeService::KGlobalAccel).expect("query"));
        assert_eq!(
            bus.calls(),
            [
                KdeService::PlasmaShell,
                KdeService::KWin,
                KdeService::KGlobalAccel
            ]
        );

        bus.reset_calls();
        assert!(bus.calls().is_empty());
    }

    #[test]
    fn a_configured_owner_can_be_removed() {
        let bus = FakeBus::all_present().without_owner(KdeService::KWin);

        assert!(bus.has_owner(KdeService::PlasmaShell).expect("query"));
        assert!(!bus.has_owner(KdeService::KWin).expect("query"));
    }

    #[test]
    fn a_failing_fake_bus_repeats_the_error_and_records_the_call() {
        let error = MorphixError::new(ErrorCode::AdapterUnavailable, "no bus");
        let bus = FakeBus::failing(error.clone());

        let returned = bus.has_owner(KdeService::PlasmaShell).expect_err("fails");
        assert_eq!(returned, error);
        assert_eq!(bus.calls(), [KdeService::PlasmaShell]);
    }

    #[test]
    fn the_fake_bus_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FakeBus>();
    }
}
