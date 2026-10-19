//! The stable error code registry.
//!
//! Codes are additive only: once released, a code never changes meaning
//! (ARCHITECTURE.md §12.1). This module is the single Rust-side source of truth, and its
//! tests fail if the registry and the documents disagree.
//!
//! Two groups exist, defined in two documents:
//!
//! - runtime and mutation codes — [ARCHITECTURE.md] §12.1;
//! - validation codes — [PACK_SPEC.md] §8.
//!
//! [ARCHITECTURE.md]: ../../../../../ARCHITECTURE.md
//! [PACK_SPEC.md]: ../../../../../PACK_SPEC.md

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A code from the registry that this build does not know.
///
/// Returned instead of panicking or silently inventing a code, so an error from a newer
/// or older producer is still parseable and reportable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownErrorCode {
    /// The unrecognised code, as received.
    pub code: String,
}

impl fmt::Display for UnknownErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown error code: {}", self.code)
    }
}

impl std::error::Error for UnknownErrorCode {}

/// Declares the code table once: enum variants, wire strings, ordering, and lookup.
///
/// Adding a code is one line plus its row in the owning document. The drift test at the
/// bottom of this file fails until both sides are updated together.
macro_rules! error_code_table {
    ($( $(#[$doc:meta])* $variant:ident = $code:literal, )*) => {
        /// A stable Morphix error code.
        ///
        /// Serialises as its exact documented string (`"E_BUSY"`), never as a number or a
        /// snake_case variant name, so logs and IPC payloads stay greppable across
        /// refactors.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum ErrorCode {
            $(
                $(#[$doc])*
                #[serde(rename = $code)]
                $variant,
            )*
        }

        impl ErrorCode {
            /// Every code in the registry, in document order: ARCHITECTURE.md §12.1 first,
            /// then PACK_SPEC.md §8.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The exact wire and log representation, for example `E_BUSY`.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $code, )*
                }
            }
        }
    };
}

error_code_table! {
    // ---------------------------------------------------------------------------------
    // Runtime and mutation codes — ARCHITECTURE.md §12.1
    // ---------------------------------------------------------------------------------
    /// Environment failed the support gate; mutations refused.
    UnsupportedPlatform = "E_UNSUPPORTED_PLATFORM",
    /// Process is running as root; mutations refused.
    RunningAsRoot = "E_RUNNING_AS_ROOT",
    /// Pack is incompatible with the detected Plasma version.
    IncompatiblePack = "E_INCOMPATIBLE_PACK",
    /// Plan uses a capability it did not declare or is not granted.
    CapabilityDenied = "E_CAPABILITY_DENIED",
    /// Resource or operation kind is outside the V0.1 target map.
    UnsupportedResource = "E_UNSUPPORTED_RESOURCE",
    /// Target path exists and is not owned by this pack.
    TargetExists = "E_TARGET_EXISTS",
    /// Write target or an ancestor is a symlink.
    SymlinkTarget = "E_SYMLINK_TARGET",
    /// Config key is marked immutable by KDE.
    KeyImmutable = "E_KEY_IMMUTABLE",
    /// A precondition changed since planning; nothing was written.
    PlanStale = "E_PLAN_STALE",
    /// Plan TTL elapsed; a new preview is required.
    PlanExpired = "E_PLAN_EXPIRED",
    /// Consent is missing or not bound to this plan hash.
    ConsentRequired = "E_CONSENT_REQUIRED",
    /// Another mutation holds the lock.
    Busy = "E_BUSY",
    /// Free space is below snapshot + install + safety margin.
    InsufficientSpace = "E_INSUFFICIENT_SPACE",
    /// A snapshot object or manifest could not be read or verified.
    SnapshotUnreadable = "E_SNAPSHOT_UNREADABLE",
    /// Snapshot manifest self-hash or object hash mismatch.
    SnapshotCorrupt = "E_SNAPSHOT_CORRUPT",
    /// An operation failed; the rollback outcome is reported separately.
    ApplyFailed = "E_APPLY_FAILED",
    /// Post-state did not match the plan.
    VerifyFailed = "E_VERIFY_FAILED",
    /// A newer change would be overwritten by rollback.
    RollbackConflict = "E_ROLLBACK_CONFLICT",
    /// Reversal could not be verified; a recovery report is attached.
    RollbackFailed = "E_ROLLBACK_FAILED",
    /// The original transaction is not committed.
    TransactionNotRollbackable = "E_TRANSACTION_NOT_ROLLBACKABLE",
    /// The pack is already applied; roll back before reapplying.
    PackAlreadyApplied = "E_PACK_ALREADY_APPLIED",
    /// An interrupted transaction could not be recovered; mutation is blocked.
    RecoveryRequired = "E_RECOVERY_REQUIRED",
    /// A required KDE interface or DBus call failed or timed out.
    AdapterUnavailable = "E_ADAPTER_UNAVAILABLE",
    /// A generated Plasma script returned a JavaScript error or backtrace.
    ScriptFailed = "E_SCRIPT_FAILED",
    /// A written key did not survive an external writer; retries exhausted.
    ConfigWriteRace = "E_CONFIG_WRITE_RACE",
    /// The pack requested panel opacity; no working mechanism exists.
    PanelOpacityUnsupported = "E_PANEL_OPACITY_UNSUPPORTED",
    /// The pack requested cursor size; no working mechanism exists.
    CursorSizeUnsupported = "E_CURSOR_SIZE_UNSUPPORTED",
    /// The change cannot be verified without a shell restart; it was not applied live.
    RestartRequired = "E_RESTART_REQUIRED",
    /// Plasma did not persist state within the quiesce timeout.
    PersistTimeout = "E_PERSIST_TIMEOUT",
    /// Engine-created panel or desktop config could not be removed.
    ResidueCleanupFailed = "E_RESIDUE_CLEANUP_FAILED",
    /// A config file exceeds the 16 MiB read cap.
    ConfigTooLarge = "E_CONFIG_TOO_LARGE",
    /// Permission denied during a targeted read or write.
    IoPermission = "E_IO_PERMISSION",
    /// Another targeted I/O failure, with context.
    Io = "E_IO",
    /// SQLite `quick_check` failed; mutations blocked.
    DbCorrupt = "E_DB_CORRUPT",
    /// The database schema is newer than this build.
    DbNewerThanApp = "E_DB_NEWER_THAN_APP",
    /// A caught panic or an impossible state; never a success.
    Internal = "E_INTERNAL",

    // ---------------------------------------------------------------------------------
    // Validation codes — PACK_SPEC.md §8
    // ---------------------------------------------------------------------------------
    /// `pack.toml` exceeds 256 KiB.
    ManifestTooLarge = "E_MANIFEST_TOO_LARGE",
    /// Not valid UTF-8.
    ManifestEncoding = "E_MANIFEST_ENCODING",
    /// TOML parse error.
    ManifestSyntax = "E_MANIFEST_SYNTAX",
    /// `schema_version` is missing or not `1`.
    SchemaVersionUnsupported = "E_SCHEMA_VERSION_UNSUPPORTED",
    /// Unknown or reserved field.
    UnknownField = "E_UNKNOWN_FIELD",
    /// Required field absent.
    MissingField = "E_MISSING_FIELD",
    /// Wrong TOML type.
    TypeMismatch = "E_TYPE_MISMATCH",
    /// Size, count, or length limit exceeded.
    LimitExceeded = "E_LIMIT_EXCEEDED",
    /// Pack ID invalid or reserved.
    InvalidId = "E_INVALID_ID",
    /// Version is not strict SemVer.
    InvalidVersion = "E_INVALID_VERSION",
    /// Asset path invalid, absolute, or outside the allowed directories.
    InvalidPath = "E_INVALID_PATH",
    /// A `..` component, or an escape after normalization.
    PathTraversal = "E_PATH_TRAVERSAL",
    /// A symlink or hardlink inside the pack.
    SymlinkRejected = "E_SYMLINK_REJECTED",
    /// Not a regular file or directory, or the executable bit is set.
    FileTypeRejected = "E_FILE_TYPE_REJECTED",
    /// A referenced asset does not exist or is empty.
    AssetMissing = "E_ASSET_MISSING",
    /// Extension or structure not allowed for the asset kind.
    AssetTypeUnsupported = "E_ASSET_TYPE_UNSUPPORTED",
    /// Bad checksum line, duplicate entry, or extra entry.
    ChecksumMalformed = "E_CHECKSUM_MALFORMED",
    /// A pack file is missing from the checksum manifest.
    ChecksumIncomplete = "E_CHECKSUM_INCOMPLETE",
    /// Content does not match the declared hash.
    ChecksumMismatch = "E_CHECKSUM_MISMATCH",
    /// A pack file is not referenced by the manifest, checksums, or docs.
    UnreferencedFile = "E_UNREFERENCED_FILE",
    /// Duplicate asset key or settings target.
    DuplicateKey = "E_DUPLICATE_KEY",
    /// Two declarations resolve to the same target.
    DuplicateTarget = "E_DUPLICATE_TARGET",
    /// Capability ID is not in the closed set.
    CapabilityUnknown = "E_CAPABILITY_UNKNOWN",
    /// A resource is used without requesting its capability.
    CapabilityUndeclared = "E_CAPABILITY_UNDECLARED",
    /// A capability is requested but nothing uses it.
    CapabilityUnused = "E_CAPABILITY_UNUSED",
    /// Settings target is not in the fixed map.
    ConfigTargetUnsupported = "E_CONFIG_TARGET_UNSUPPORTED",
    /// Key is not on the allowlist.
    ConfigKeyNotAllowed = "E_CONFIG_KEY_NOT_ALLOWED",
    /// Value type or range rejected by the allowlist.
    ValueInvalid = "E_VALUE_INVALID",
    /// Effect ID is not allowlisted.
    EffectNotAllowed = "E_EFFECT_NOT_ALLOWED",
    /// Component or action is not allowlisted, or the chord is malformed.
    ShortcutNotAllowed = "E_SHORTCUT_NOT_ALLOWED",
    /// Desktop is not `kde`, or the Plasma range admits a version below 6.0.
    CompatibilityInvalid = "E_COMPATIBILITY_INVALID",
    /// Dependency ID or range invalid.
    DependencyInvalid = "E_DEPENDENCY_INVALID",
    /// A cycle among packs.
    DependencyCycle = "E_DEPENDENCY_CYCLE",
    /// A dependency is not installed or applied at plan time.
    DependencyMissing = "E_DEPENDENCY_MISSING",
    /// `README.md` or `ATTRIBUTION.md` is missing.
    RequiredFileMissing = "E_REQUIRED_FILE_MISSING",
    /// The same ID and version is already installed.
    PackAlreadyInstalled = "E_PACK_ALREADY_INSTALLED",
    /// The same ID with a different version is installed (V0.1 refuses upgrades).
    PackIdConflict = "E_PACK_ID_CONFLICT",
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ErrorCode {
    type Err = UnknownErrorCode;

    /// Parses an exact wire code. Case-sensitive by design: codes are compared, diffed,
    /// and grepped, so a near-miss must fail loudly.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == s)
            .ok_or_else(|| UnknownErrorCode { code: s.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    // Tests fail loudly: `expect` with a message is allowed here even though the library
    // target denies it (CONTRIBUTING.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{ErrorCode, UnknownErrorCode};
    use std::collections::BTreeSet;
    use std::str::FromStr;

    /// The documents this registry is derived from, read at compile time so the test fails
    /// in CI when a code is added to one side only.
    const ARCHITECTURE: &str = include_str!("../../../../ARCHITECTURE.md");
    const PACK_SPEC: &str = include_str!("../../../../PACK_SPEC.md");

    /// Extracts `| \`E_...\` |` codes from the table that follows a heading.
    fn codes_in_table(doc: &str, heading: &str, end: &str) -> BTreeSet<String> {
        let start = doc.find(heading).expect("heading not found");
        let rest = &doc[start..];
        let stop = rest.find(end).expect("end marker not found");
        let section = &rest[..stop];
        section
            .lines()
            .filter_map(|line| {
                let cell = line.strip_prefix("| `")?;
                let (code, _) = cell.split_once('`')?;
                code.starts_with("E_").then(|| code.to_owned())
            })
            .collect()
    }

    fn documented_codes() -> BTreeSet<String> {
        let mut codes = codes_in_table(ARCHITECTURE, "### 12.1 Error code registry", "## 13.");
        assert_eq!(
            codes.len(),
            36,
            "ARCHITECTURE §12.1 runtime code count changed"
        );
        codes.extend(codes_in_table(
            PACK_SPEC,
            "### Validation error codes",
            "Runtime codes such as",
        ));
        codes
    }

    #[test]
    fn registry_matches_the_documents() {
        let documented = documented_codes();
        let implemented: BTreeSet<String> = ErrorCode::ALL
            .iter()
            .map(|code| code.as_str().to_owned())
            .collect();

        assert_eq!(
            implemented.len(),
            ErrorCode::ALL.len(),
            "the registry contains duplicate code strings"
        );
        let missing: Vec<_> = documented.difference(&implemented).collect();
        let extra: Vec<_> = implemented.difference(&documented).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "registry and documents disagree; missing from code: {missing:?}; not documented: {extra:?}"
        );
        assert_eq!(
            ErrorCode::ALL.len(),
            73,
            "registry size changed; update the documents too"
        );
    }

    #[test]
    fn every_code_round_trips_through_json() {
        for code in ErrorCode::ALL {
            let json = serde_json::to_string(code).expect("code must serialise");
            assert_eq!(
                json,
                format!("\"{}\"", code.as_str()),
                "wire form must be the code string"
            );

            let parsed: ErrorCode = serde_json::from_str(&json).expect("code must deserialise");
            assert_eq!(parsed, *code);
            assert_eq!(ErrorCode::from_str(code.as_str()), Ok(*code));
        }
    }

    #[test]
    fn every_code_displays_as_its_registry_string() {
        for code in ErrorCode::ALL {
            assert_eq!(code.to_string(), code.as_str());
        }
    }

    #[test]
    fn unknown_codes_are_rejected_without_panicking() {
        for input in ["", "E_", "e_busy", "E_BUSY ", "E_NOT_A_REAL_CODE", "42"] {
            let parsed = ErrorCode::from_str(input);
            assert_eq!(
                parsed,
                Err(UnknownErrorCode {
                    code: input.to_owned()
                }),
                "{input:?} must not resolve to a code"
            );

            // Serde must reject it too: an unknown code may never deserialise into a
            // silently different one.
            let json = format!("\"{input}\"");
            assert!(
                serde_json::from_str::<ErrorCode>(&json).is_err(),
                "{json} must be rejected"
            );
        }
    }
}
