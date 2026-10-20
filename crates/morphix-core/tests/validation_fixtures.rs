//! One fixture per documented validation code (P2-14, AC-19).
//!
//! Fixtures are built on disk by [`support::PackBuilder`]. Each row asserts that the
//! expected code appears in the aggregated report; other aggregated issues are allowed,
//! because the validator deliberately reports everything it finds.
//!
//! Three codes from PACK_SPEC.md §8 are not produced by `validate_directory`:
//! `E_DEPENDENCY_MISSING` is a plan-time check (PACK_SPEC.md §5.12, Phase 3), and
//! `E_PACK_ALREADY_INSTALLED` / `E_PACK_ID_CONFLICT` are install-state checks. The latter
//! two are covered by `tests/install_local_pack.rs`; the accounting test below makes the
//! deferral explicit rather than silently skipping them.

#![allow(clippy::vec_init_then_push)]

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use morphix_core::errors::ErrorCode;
use morphix_core::packs::report::ValidationOutcome;
use morphix_core::packs::validator::validate_directory;
use support::{PackBuilder, write_checksums};

const CHECKSUM_SECTION: &str =
    "\n[integrity]\nalgorithm = \"sha256\"\nmanifest = \"checksums.sha256\"\n";

/// Validation codes that only the install service can produce (covered elsewhere).
const INSTALL_STATE_CODES: &[ErrorCode] =
    &[ErrorCode::PackAlreadyInstalled, ErrorCode::PackIdConflict];

/// The one documented code that needs the Phase 3 planner.
const PLAN_TIME_CODES: &[ErrorCode] = &[ErrorCode::DependencyMissing];

/// Builds every fixture, keyed by the code it must produce.
fn fixtures(root: &Path) -> Vec<(ErrorCode, PackBuilder)> {
    let mut fixtures = Vec::new();

    fixtures.push((
        ErrorCode::ManifestTooLarge,
        PackBuilder::new(root.join("manifest-too-large")).raw_manifest(vec![b'#'; 256 * 1024 + 1]),
    ));
    fixtures.push((
        ErrorCode::ManifestEncoding,
        PackBuilder::new(root.join("manifest-encoding"))
            .raw_manifest(b"schema_version = 1\n\xff\n".to_vec()),
    ));
    fixtures.push((
        ErrorCode::ManifestSyntax,
        PackBuilder::new(root.join("manifest-syntax"))
            .raw_manifest(b"schema_version = 1\n[pack\n".to_vec()),
    ));
    fixtures.push((
        ErrorCode::SchemaVersionUnsupported,
        PackBuilder::new(root.join("schema-version"))
            .replace("schema_version = 1", "schema_version = 2"),
    ));
    fixtures.push((
        ErrorCode::UnknownField,
        PackBuilder::new(root.join("unknown-field"))
            .replace("[pack]", "unexpected_field = true\n\n[pack]"),
    ));
    fixtures.push((
        ErrorCode::MissingField,
        PackBuilder::new(root.join("missing-field")).replace("name = \"Fixture Pack\"\n", ""),
    ));
    fixtures.push((
        ErrorCode::TypeMismatch,
        PackBuilder::new(root.join("type-mismatch"))
            .replace("schema_version = 1", "schema_version = \"1\""),
    ));
    let mut too_many_previews = String::new();
    for index in 0..9 {
        too_many_previews.push_str(&format!(
            "\n[[previews]]\nimage = \"previews/icon.png\"\ncaption = \"Preview {index}\"\n"
        ));
    }
    fixtures.push((
        ErrorCode::LimitExceeded,
        PackBuilder::new(root.join("limit-exceeded"))
            .replace(
                "\n[[previews]]\nimage = \"previews/icon.png\"\ncaption = \"Fixture preview\"\n",
                "",
            )
            .append(&too_many_previews),
    ));
    fixtures.push((
        ErrorCode::InvalidId,
        PackBuilder::new(root.join("invalid-id")).replace("dev.example.fixture", "dev.Example.Bad"),
    ));
    fixtures.push((
        ErrorCode::InvalidVersion,
        PackBuilder::new(root.join("invalid-version"))
            .replace("version = \"1.0.0\"", "version = \"1.0\""),
    ));
    fixtures.push((
        ErrorCode::InvalidPath,
        PackBuilder::new(root.join("invalid-path")).replace(
            "wall = \"wallpapers/wall.png\"",
            "wall = \"/wallpapers/wall.png\"",
        ),
    ));
    fixtures.push((
        ErrorCode::PathTraversal,
        PackBuilder::new(root.join("path-traversal")).replace(
            "wall = \"wallpapers/wall.png\"",
            "wall = \"wallpapers/../wall.png\"",
        ),
    ));
    fixtures.push((
        ErrorCode::SymlinkRejected,
        PackBuilder::new(root.join("symlink")).symlink("themes/link.txt", "../../README.md"),
    ));
    fixtures.push((
        ErrorCode::FileTypeRejected,
        PackBuilder::new(root.join("executable")).mode("wallpapers/wall.png", 0o755),
    ));
    fixtures.push((
        ErrorCode::AssetMissing,
        PackBuilder::new(root.join("asset-missing")).replace(
            "wall = \"wallpapers/wall.png\"",
            "wall = \"wallpapers/missing.png\"",
        ),
    ));
    fixtures.push((
        ErrorCode::AssetTypeUnsupported,
        PackBuilder::new(root.join("asset-type"))
            .remove_file("wallpapers/wall.png")
            .file("wallpapers/wall.txt", b"not an image")
            .replace(
                "wall = \"wallpapers/wall.png\"",
                "wall = \"wallpapers/wall.txt\"",
            ),
    ));
    fixtures.push((
        ErrorCode::ChecksumMalformed,
        PackBuilder::new(root.join("checksum-malformed")).append(CHECKSUM_SECTION),
    ));
    fixtures.push((
        ErrorCode::ChecksumIncomplete,
        PackBuilder::new(root.join("checksum-incomplete")).append(CHECKSUM_SECTION),
    ));
    fixtures.push((
        ErrorCode::ChecksumMismatch,
        PackBuilder::new(root.join("checksum-mismatch")).append(CHECKSUM_SECTION),
    ));
    fixtures.push((
        ErrorCode::UnreferencedFile,
        PackBuilder::new(root.join("unreferenced")).file("themes/stray.txt", b"stray file"),
    ));
    fixtures.push((
        ErrorCode::DuplicateKey,
        PackBuilder::new(root.join("duplicate-key")).replace(
            "requires = [\"appearance.wallpaper\"]",
            "requires = [\"appearance.wallpaper\", \"appearance.wallpaper\"]",
        ),
    ));
    fixtures.push((
        ErrorCode::DuplicateTarget,
        PackBuilder::new(root.join("duplicate-target"))
            .replace(
                "requires = [\"appearance.wallpaper\"]",
                "requires = [\"appearance.wallpaper\", \"appearance.color_scheme\"]",
            )
            .replace(
                "wallpaper = \"wall\"",
                "wallpaper = \"wall\"\ncolor_scheme = \"scheme\"",
            )
            .replace(
                "[[previews]]",
                "[assets.color_schemes]\nscheme = \"themes/scheme.colors\"\n\n[[previews]]",
            )
            .file("themes/scheme.colors", b"[General]\nName=Fixture\n")
            .append(
                "\n[[settings]]\ntarget = \"kdeglobals\"\ngroup = \"General\"\n\
                 key = \"ColorScheme\"\nvalue = \"Fixture\"\n",
            ),
    ));
    fixtures.push((
        ErrorCode::CapabilityUnknown,
        PackBuilder::new(root.join("capability-unknown"))
            .replace("appearance.wallpaper", "appearance.invented"),
    ));
    fixtures.push((
        ErrorCode::CapabilityUndeclared,
        PackBuilder::new(root.join("capability-undeclared"))
            .replace("requires = [\"appearance.wallpaper\"]", "requires = []"),
    ));
    fixtures.push((
        ErrorCode::CapabilityUnused,
        PackBuilder::new(root.join("capability-unused")).replace(
            "requires = [\"appearance.wallpaper\"]",
            "requires = [\"appearance.wallpaper\", \"appearance.icons\"]",
        ),
    ));
    fixtures.push((
        ErrorCode::ConfigTargetUnsupported,
        PackBuilder::new(root.join("config-target")).append(
            "\n[[settings]]\ntarget = \"plasmarc\"\ngroup = \"Theme\"\n\
             key = \"name\"\nvalue = \"breeze\"\n",
        ),
    ));
    fixtures.push((
        ErrorCode::ConfigKeyNotAllowed,
        PackBuilder::new(root.join("config-key")).append(
            "\n[[settings]]\ntarget = \"kdeglobals\"\ngroup = \"General\"\n\
             key = \"NotAllowlisted\"\nvalue = \"x\"\n",
        ),
    ));
    fixtures.push((
        ErrorCode::ValueInvalid,
        PackBuilder::new(root.join("value-invalid")).append(
            "\n[[settings]]\ntarget = \"kdeglobals\"\ngroup = \"General\"\n\
             key = \"font\"\nvalue = 42\n",
        ),
    ));
    fixtures.push((
        ErrorCode::EffectNotAllowed,
        PackBuilder::new(root.join("effect-not-allowed"))
            .append("\n[[kwin.effects]]\nid = \"invented.effect\"\nenabled = true\n"),
    ));
    fixtures.push((
        ErrorCode::ShortcutNotAllowed,
        PackBuilder::new(root.join("shortcut-not-allowed")).append(
            "\n[[shortcuts]]\ncomponent = \"kwin\"\naction = \"Do Something Else\"\n\
             keys = [\"Meta+Q\"]\n",
        ),
    ));
    fixtures.push((
        ErrorCode::CompatibilityInvalid,
        PackBuilder::new(root.join("compatibility"))
            .replace("desktop = \"kde\"", "desktop = \"gnome\""),
    ));
    fixtures.push((
        ErrorCode::DependencyInvalid,
        PackBuilder::new(root.join("dependency-invalid"))
            .append("\n[[dependencies.packs]]\nid = \"Not_A_Pack_Id\"\nversion = \"1.0.0\"\n"),
    ));
    fixtures.push((
        ErrorCode::DependencyCycle,
        PackBuilder::new(root.join("dependency-cycle")).append(
            "\n[[dependencies.packs]]\nid = \"dev.example.fixture\"\nversion = \">=1.0.0\"\n",
        ),
    ));
    fixtures.push((
        ErrorCode::RequiredFileMissing,
        PackBuilder::new(root.join("required-file")).remove_file("README.md"),
    ));

    fixtures
}

fn prepare_checksum_fixture(code: ErrorCode, root: &Path) {
    match code {
        ErrorCode::ChecksumMalformed => {
            std::fs::write(root.join("checksums.sha256"), b"not a checksum line\n")
                .expect("malformed checksums");
        }
        ErrorCode::ChecksumIncomplete => {
            write_checksums(root, &["README.md"], || {});
        }
        ErrorCode::ChecksumMismatch => {
            write_checksums(root, &[], || {
                std::fs::write(root.join("wallpapers/wall.png"), b"changed after checksum")
                    .expect("mutate wallpaper");
            });
        }
        _ => {}
    }
}

fn validate(builder: &PackBuilder) -> ValidationOutcome {
    builder.build();
    validate_directory(builder.root()).expect("validation runs")
}

#[test]
fn every_fixture_produces_its_documented_code() {
    let temp = tempfile::tempdir().expect("temp");
    let fixtures = fixtures(temp.path());
    assert_eq!(fixtures.len(), 34, "one fixture per reachable code");

    let mut observed = BTreeSet::new();
    for (expected, builder) in fixtures {
        let root = builder.build();
        prepare_checksum_fixture(expected, root);
        let outcome = validate_directory(root)
            .unwrap_or_else(|error| panic!("{expected}: validation I/O failed: {error}"));
        let codes: Vec<_> = outcome.issues.iter().map(|issue| issue.code).collect();
        assert!(
            codes.contains(&expected),
            "{expected} missing from {codes:?} ({:?})",
            outcome
                .issues
                .iter()
                .map(|issue| (&issue.code, &issue.message))
                .collect::<Vec<_>>()
        );
        observed.extend(codes);
    }

    for code in &observed {
        assert!(
            is_validation_code(*code),
            "a fixture produced a non-validation code: {code}"
        );
    }
}

#[test]
fn every_documented_validation_code_is_accounted_for() {
    let temp = tempfile::tempdir().expect("temp");
    let documented = documented_validation_codes();
    let fixture_codes: BTreeSet<ErrorCode> = fixtures(temp.path())
        .into_iter()
        .map(|(code, _)| code)
        .collect();

    for code in documented {
        let covered = fixture_codes.contains(&code)
            || INSTALL_STATE_CODES.contains(&code)
            || PLAN_TIME_CODES.contains(&code);
        assert!(
            covered,
            "{code} is not covered by a fixture or an explicit deferral"
        );
    }

    // The base pack must be valid; otherwise every fixture above proves nothing.
    let base = PackBuilder::new(temp.path().join("valid-base"));
    let outcome = validate(&base);
    assert!(
        outcome.is_valid(),
        "base fixture is invalid: {:?}",
        outcome
            .issues
            .iter()
            .map(|issue| (&issue.code, &issue.path, &issue.field))
            .collect::<Vec<_>>()
    );
}

/// PACK_SPEC.md §8's error-code table, in document order.
fn documented_validation_codes() -> Vec<ErrorCode> {
    use ErrorCode::{
        AssetMissing, AssetTypeUnsupported, CapabilityUndeclared, CapabilityUnknown,
        CapabilityUnused, ChecksumIncomplete, ChecksumMalformed, ChecksumMismatch,
        CompatibilityInvalid, ConfigKeyNotAllowed, ConfigTargetUnsupported, DependencyCycle,
        DependencyInvalid, DependencyMissing, DuplicateKey, DuplicateTarget, EffectNotAllowed,
        FileTypeRejected, InvalidId, InvalidPath, InvalidVersion, LimitExceeded, ManifestEncoding,
        ManifestSyntax, ManifestTooLarge, MissingField, PackAlreadyInstalled, PackIdConflict,
        PathTraversal, RequiredFileMissing, SchemaVersionUnsupported, ShortcutNotAllowed,
        SymlinkRejected, TypeMismatch, UnknownField, UnreferencedFile, ValueInvalid,
    };
    vec![
        ManifestTooLarge,
        ManifestEncoding,
        ManifestSyntax,
        SchemaVersionUnsupported,
        UnknownField,
        MissingField,
        TypeMismatch,
        LimitExceeded,
        InvalidId,
        InvalidVersion,
        InvalidPath,
        PathTraversal,
        SymlinkRejected,
        FileTypeRejected,
        AssetMissing,
        AssetTypeUnsupported,
        ChecksumMalformed,
        ChecksumIncomplete,
        ChecksumMismatch,
        UnreferencedFile,
        DuplicateKey,
        DuplicateTarget,
        CapabilityUnknown,
        CapabilityUndeclared,
        CapabilityUnused,
        ConfigTargetUnsupported,
        ConfigKeyNotAllowed,
        ValueInvalid,
        EffectNotAllowed,
        ShortcutNotAllowed,
        CompatibilityInvalid,
        DependencyInvalid,
        DependencyCycle,
        DependencyMissing,
        RequiredFileMissing,
        PackAlreadyInstalled,
        PackIdConflict,
    ]
}

fn is_validation_code(code: ErrorCode) -> bool {
    documented_validation_codes().contains(&code)
}
