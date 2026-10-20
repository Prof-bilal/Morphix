//! Semantic validation pipeline (PACK_SPEC.md §8, P2-05/P2-08/P2-09/P2-10).
//!
//! Validation is read-only and aggregates up to [`MAX_VALIDATION_ISSUES`] problems. Any
//! issue means installation is refused and nothing is written. Checks are ordered like the
//! documented pipeline: tree safety, field limits, capabilities, assets and structure,
//! cross-references, compatibility, dependencies, checksums, and the unreferenced-file
//! audit.

use std::collections::BTreeSet;
use std::path::Path;

use semver::VersionReq;

use crate::core::allowlist::{self, ConfigTarget, ConfigValueType};
use crate::core::capability::Capability;
use crate::core::ids::{InstalledName, PackId};
use crate::core::resource::AssetKind;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::tree::{EntryKind, TreeSnapshot};

use super::compatibility::PlasmaConstraint;
use super::integrity::{self, CHECKSUM_FILE};
use super::manifest::{Manifest, SettingValue};
use super::parser::{MANIFEST_FILE, MAX_MANIFEST_BYTES, parse_manifest};
use super::path::{is_under, validate_asset_path, validate_relative};
use super::report::{
    DeclaredChange, IntegritySource, MAX_VALIDATION_ISSUES, ValidationIssue, ValidationOutcome,
};
use super::structure;

/// One MiB, the icon cap.
const ICON_LIMIT: u64 = 1024 * 1024;
/// Five MiB, the per-preview cap.
const PREVIEW_LIMIT: u64 = 5 * 1024 * 1024;
/// Fifty MiB, the single-file cap.
const FILE_LIMIT: u64 = 50 * 1024 * 1024;
/// Two hundred MiB, the total cap.
const TOTAL_LIMIT: u64 = 200 * 1024 * 1024;
/// Two thousand files.
const FILE_COUNT_LIMIT: usize = 2000;

/// Validates a pack directory and returns every problem found.
///
/// # Errors
///
/// Returns a typed I/O error only when the tree cannot be inspected; every pack-content
/// problem is returned in [`ValidationOutcome::issues`].
pub fn validate_directory(root: &Path) -> Result<ValidationOutcome> {
    let root_metadata = std::fs::symlink_metadata(root)
        .map_err(|error| crate::fs::sandbox::io_error(root, &error))?;
    let tree = crate::fs::tree::snapshot(root)?;
    let content_hash = tree.content_hash();
    let mut issues = Vec::new();

    if root_metadata.file_type().is_symlink() {
        issues.push(ValidationIssue::new(
            ErrorCode::SymlinkRejected,
            "the pack directory itself must not be a symbolic link",
        ));
    } else if !root_metadata.is_dir() {
        issues.push(ValidationIssue::new(
            ErrorCode::FileTypeRejected,
            "a pack source must be a directory",
        ));
    }

    let manifest_path = root.join(MANIFEST_FILE);
    let metadata = match std::fs::symlink_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            issues.push(
                ValidationIssue::new(ErrorCode::RequiredFileMissing, "the pack has no pack.toml")
                    .at_path(MANIFEST_FILE),
            );
            return Ok(outcome(None, None, content_hash, tree, 0, issues));
        }
        Err(error) => return Err(crate::fs::sandbox::io_error(&manifest_path, &error)),
    };

    if metadata.file_type().is_symlink() {
        issues.push(
            ValidationIssue::new(
                ErrorCode::SymlinkRejected,
                "pack.toml must not be a symbolic link",
            )
            .at_path(MANIFEST_FILE),
        );
        return Ok(outcome(None, None, content_hash, tree, 0, issues));
    }
    if !metadata.is_file() {
        issues.push(
            ValidationIssue::new(ErrorCode::FileTypeRejected, "pack.toml must be a file")
                .at_path(MANIFEST_FILE),
        );
        return Ok(outcome(None, None, content_hash, tree, 0, issues));
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        issues.push(
            ValidationIssue::new(
                ErrorCode::ManifestTooLarge,
                "pack.toml is larger than the 256 KiB limit",
            )
            .at_path(MANIFEST_FILE),
        );
        return Ok(outcome(
            None,
            None,
            content_hash,
            tree,
            metadata.len(),
            issues,
        ));
    }

    let bytes = std::fs::read(&manifest_path)
        .map_err(|error| crate::fs::sandbox::io_error(&manifest_path, &error))?;
    let manifest_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let parsed = match parse_manifest(&bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            issues.push(issue_from_error(&error));
            return Ok(outcome(
                None,
                None,
                content_hash,
                tree,
                manifest_bytes,
                issues,
            ));
        }
    };

    let mut checker = Checker::new(root, &tree, &parsed.manifest);
    checker.run();
    let Checker {
        issues: mut checker_issues,
        changes,
        integrity_source,
        ..
    } = checker;
    checker_issues.truncate(MAX_VALIDATION_ISSUES);

    Ok(ValidationOutcome {
        manifest: Some(parsed.manifest),
        manifest_hash: Some(parsed.manifest_hash),
        content_hash,
        tree,
        manifest_bytes,
        issues: checker_issues,
        declared_changes: changes,
        integrity_source,
    })
}

fn outcome(
    manifest: Option<Manifest>,
    manifest_hash: Option<String>,
    content_hash: String,
    tree: TreeSnapshot,
    manifest_bytes: u64,
    issues: Vec<ValidationIssue>,
) -> ValidationOutcome {
    ValidationOutcome {
        manifest,
        manifest_hash,
        content_hash,
        tree,
        manifest_bytes,
        issues,
        declared_changes: Vec::new(),
        integrity_source: IntegritySource::GeneratedLocal,
    }
}

fn issue_from_error(error: &MorphixError) -> ValidationIssue {
    let mut issue = ValidationIssue::new(error.code, error.message.clone());
    for entry in &error.context {
        match entry.key.as_str() {
            "field" => issue.field = Some(entry.value.clone()),
            "path" => issue.path = Some(entry.value.clone()),
            _ => {}
        }
    }
    issue
}

struct Checker<'a> {
    root: &'a Path,
    tree: &'a TreeSnapshot,
    manifest: &'a Manifest,
    issues: Vec<ValidationIssue>,
    declared: BTreeSet<Capability>,
    used: BTreeSet<Capability>,
    undeclared_reported: BTreeSet<Capability>,
    referenced: BTreeSet<String>,
    changes: Vec<DeclaredChange>,
    integrity_source: IntegritySource,
}

impl<'a> Checker<'a> {
    fn new(root: &'a Path, tree: &'a TreeSnapshot, manifest: &'a Manifest) -> Self {
        let mut referenced = BTreeSet::new();
        for name in [MANIFEST_FILE, "README.md", "ATTRIBUTION.md", CHECKSUM_FILE] {
            referenced.insert(name.to_owned());
        }
        Self {
            root,
            tree,
            manifest,
            issues: Vec::new(),
            declared: BTreeSet::new(),
            used: BTreeSet::new(),
            undeclared_reported: BTreeSet::new(),
            referenced,
            changes: Vec::new(),
            integrity_source: IntegritySource::GeneratedLocal,
        }
    }

    fn run(&mut self) {
        self.check_tree();
        self.check_metadata();
        self.check_capabilities();
        self.check_assets();
        self.check_appearance();
        self.check_settings();
        self.check_effects();
        self.check_shortcuts();
        self.check_panel();
        self.check_desktop_behavior();
        self.check_compatibility();
        self.check_dependencies();
        self.check_checksums();
        self.check_references();
        self.check_capability_usage();
    }

    fn issue(&mut self, code: ErrorCode, message: impl Into<String>) -> ValidationIssue {
        let issue = ValidationIssue::new(code, message);
        if self.issues.len() < MAX_VALIDATION_ISSUES {
            self.issues.push(issue.clone());
        }
        issue
    }

    fn push(&mut self, issue: ValidationIssue) {
        if self.issues.len() < MAX_VALIDATION_ISSUES {
            self.issues.push(issue);
        }
    }

    fn use_capability(&mut self, capability: Capability) {
        if self.declared.contains(&capability) {
            self.used.insert(capability);
        } else if self.undeclared_reported.insert(capability) {
            self.push(
                ValidationIssue::new(
                    ErrorCode::CapabilityUndeclared,
                    "a declared resource needs a capability the pack did not request",
                )
                .at_field(capability.as_str()),
            );
        }
    }

    fn check_tree(&mut self) {
        for entry in &self.tree.entries {
            if let Err(error) = validate_relative(&entry.relative) {
                let mut issue = ValidationIssue::new(error.code, error.message);
                issue.path = Some(entry.relative.clone());
                self.push(issue);
            }
            match entry.kind {
                EntryKind::Symlink => self.push(
                    ValidationIssue::new(
                        ErrorCode::SymlinkRejected,
                        "symbolic links are not allowed in a pack",
                    )
                    .at_path(&entry.relative),
                ),
                EntryKind::Other => self.push(
                    ValidationIssue::new(
                        ErrorCode::FileTypeRejected,
                        "devices, sockets, and FIFOs are not allowed in a pack",
                    )
                    .at_path(&entry.relative),
                ),
                EntryKind::File if entry.links > 1 => self.push(
                    ValidationIssue::new(
                        ErrorCode::SymlinkRejected,
                        "hard-linked files are not allowed in a pack",
                    )
                    .at_path(&entry.relative),
                ),
                EntryKind::File if entry.mode & 0o111 != 0 => self.push(
                    ValidationIssue::new(
                        ErrorCode::FileTypeRejected,
                        "executable files are not allowed in a pack",
                    )
                    .at_path(&entry.relative),
                ),
                EntryKind::File if entry.size > FILE_LIMIT => self.push(
                    ValidationIssue::new(
                        ErrorCode::LimitExceeded,
                        "a pack file is larger than the 50 MiB limit",
                    )
                    .at_path(&entry.relative),
                ),
                _ => {}
            }
        }

        if self.tree.file_count() > FILE_COUNT_LIMIT {
            self.issue(
                ErrorCode::LimitExceeded,
                "the pack contains more than 2,000 files",
            );
        }
        if self.tree.total_bytes > TOTAL_LIMIT {
            self.issue(
                ErrorCode::LimitExceeded,
                "the pack is larger than the 200 MiB limit",
            );
        }
    }

    fn check_metadata(&mut self) {
        let pack = self.manifest.pack.clone();
        let name_length = pack.name.chars().count();
        if pack.name.trim() != pack.name || name_length == 0 || name_length > 80 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "the pack name must be trimmed and 1..80 characters",
                )
                .at_field("pack.name"),
            );
        }
        if pack.name.chars().any(char::is_control) {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "the pack name contains a control character",
                )
                .at_field("pack.name"),
            );
        }

        let summary_length = pack.summary.chars().count();
        if summary_length == 0 || summary_length > 200 || pack.summary.contains(['\n', '\r']) {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "the summary must be a single line of 1..200 characters",
                )
                .at_field("pack.summary"),
            );
        }

        let description_length = pack.description.chars().count();
        if description_length == 0 || description_length > 8192 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "the description must be 1..8192 characters",
                )
                .at_field("pack.description"),
            );
        }

        if pack.authors.is_empty() || pack.authors.len() > 16 {
            self.push(
                ValidationIssue::new(ErrorCode::ValueInvalid, "a pack must have 1..16 authors")
                    .at_field("pack.authors"),
            );
        }
        for author in &pack.authors {
            let length = author.chars().count();
            if length == 0 || length > 128 || author.chars().any(char::is_control) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "an author name must be 1..128 characters without control characters",
                    )
                    .at_field("pack.authors"),
                );
            }
        }

        if !is_spdx_expression(&pack.license) {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "the license must be a syntactically valid SPDX expression",
                )
                .at_field("pack.license"),
            );
        }

        if let Some(homepage) = &pack.homepage
            && (!homepage.starts_with("https://")
                || homepage.len() > 2048
                || homepage.chars().any(char::is_whitespace))
        {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "homepage must be an https:// URL without whitespace",
                )
                .at_field("pack.homepage"),
            );
        }

        if validate_asset_path(&pack.icon).is_ok() {
            self.referenced.insert(pack.icon.clone());
        }
        for issue in structure::check_image(&pack.icon, self.tree, ICON_LIMIT, true) {
            self.push(issue);
        }

        if self.manifest.previews.is_empty() || self.manifest.previews.len() > 8 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack must declare 1..8 previews",
                )
                .at_field("previews"),
            );
        }
        for preview in &self.manifest.previews {
            if validate_asset_path(&preview.image).is_ok() {
                self.referenced.insert(preview.image.clone());
            }
            for issue in structure::check_image(&preview.image, self.tree, PREVIEW_LIMIT, true) {
                self.push(issue);
            }
            let caption_length = preview.caption.chars().count();
            if caption_length == 0
                || caption_length > 120
                || preview.caption.chars().any(char::is_control)
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a preview caption must be 1..120 characters",
                    )
                    .at_path(&preview.image),
                );
            }
        }

        for required in ["README.md", "ATTRIBUTION.md"] {
            match self.tree.entry(required) {
                Some(entry) if entry.kind == EntryKind::File && entry.size > 0 => {}
                _ => self.push(
                    ValidationIssue::new(
                        ErrorCode::RequiredFileMissing,
                        format!("the required document {required} is missing or empty"),
                    )
                    .at_path(required),
                ),
            }
        }
    }

    fn check_capabilities(&mut self) {
        let requires = self.manifest.capabilities.requires.clone();
        if requires.is_empty() || requires.len() > 9 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack must request 1..9 capabilities",
                )
                .at_field("capabilities.requires"),
            );
        }
        for id in requires {
            match Capability::from_id(id.as_str()) {
                Some(capability) => {
                    if !self.declared.insert(capability) {
                        self.push(
                            ValidationIssue::new(
                                ErrorCode::DuplicateKey,
                                "a capability is requested more than once",
                            )
                            .at_field("capabilities.requires"),
                        );
                    }
                }
                None => self.push(
                    ValidationIssue::new(
                        ErrorCode::CapabilityUnknown,
                        format!("unknown capability: {id}"),
                    )
                    .at_field("capabilities.requires"),
                ),
            }
        }
    }

    fn check_assets(&mut self) {
        let mut instalments = Vec::new();
        for (kind, key, path) in self.manifest.assets_iter() {
            instalments.push((kind, key.clone(), path.to_owned()));
        }

        for (kind, key, path) in instalments {
            let map = self.manifest.assets.for_kind(kind);
            if map.len() > 64 {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::LimitExceeded,
                        "an asset kind may declare at most 64 keys",
                    )
                    .at_field(kind.manifest_table()),
                );
            }

            match validate_asset_path(&path) {
                Ok(validated) => {
                    self.referenced.insert(validated.to_owned());
                    if !path_matches_kind(kind, &path) {
                        self.push(
                            ValidationIssue::new(
                                ErrorCode::AssetTypeUnsupported,
                                format!(
                                    "a {} asset must live under the documented directory",
                                    kind.as_str()
                                ),
                            )
                            .at_path(&path),
                        );
                    }

                    // Structural checks may read a file (the `.colors` header), so they
                    // only run after the path grammar has been validated.
                    for issue in structure::check_asset(kind, self.root, &path, self.tree) {
                        self.push(issue);
                    }
                    if kind.is_directory() {
                        for reference in structure::directory_references(self.tree, &path) {
                            self.referenced.insert(reference);
                        }
                    }
                }
                Err(error) => {
                    let mut issue = ValidationIssue::new(error.code, error.message);
                    issue.path = Some(path.clone());
                    self.push(issue);
                }
            }

            if let Err(error) = InstalledName::derive(&self.manifest.pack.id, &key) {
                let mut issue = ValidationIssue::new(error.code, error.message);
                issue.path = Some(path.clone());
                self.push(issue);
            }

            let capability = capability_for_asset(kind);
            self.use_capability(capability);
            self.changes.push(DeclaredChange::new(
                capability,
                "install_asset",
                format!("{}:{} → {}", kind.as_str(), key, path),
            ));
        }
    }

    fn check_appearance(&mut self) {
        let Some(appearance) = self.manifest.appearance.clone() else {
            return;
        };
        if appearance.is_empty() {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "[appearance] must contain at least one field",
                )
                .at_field("appearance"),
            );
            return;
        }

        let selections = [
            (
                appearance.color_scheme.as_deref(),
                AssetKind::ColorScheme,
                "appearance.color_scheme",
                "write_config",
                Capability::ColorScheme,
            ),
            (
                appearance.icon_theme.as_deref(),
                AssetKind::IconTheme,
                "appearance.icon_theme",
                "write_config",
                Capability::Icons,
            ),
            (
                appearance.cursor_theme.as_deref(),
                AssetKind::CursorTheme,
                "appearance.cursor_theme",
                "write_config",
                Capability::Cursors,
            ),
            (
                appearance.plasma_style.as_deref(),
                AssetKind::PlasmaStyle,
                "appearance.plasma_style",
                "write_config",
                Capability::PlasmaStyle,
            ),
            (
                appearance.window_decoration.as_deref(),
                AssetKind::WindowDecoration,
                "appearance.window_decoration",
                "write_config",
                Capability::WindowDecorations,
            ),
            (
                appearance.wallpaper.as_deref(),
                AssetKind::Wallpaper,
                "appearance.wallpaper",
                "set_wallpaper",
                Capability::Wallpaper,
            ),
        ];

        for (selection, kind, field, operation, capability) in selections {
            let Some(key) = selection else { continue };
            let map = self.manifest.assets.for_kind(kind);
            let Some(path) = map
                .iter()
                .find(|(candidate, _)| candidate.as_str() == key)
                .map(|(_, path)| path)
            else {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::AssetMissing,
                        format!(
                            "appearance references an undefined {} asset key",
                            kind.as_str()
                        ),
                    )
                    .at_field(field),
                );
                continue;
            };
            self.use_capability(capability);
            self.changes.push(DeclaredChange::new(
                capability,
                operation,
                format!("{}:{} ({})", kind.as_str(), key, path),
            ));
        }

        if let Some(size) = appearance.cursor_size {
            if !(16..=128).contains(&size) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "cursor_size must be between 16 and 128",
                    )
                    .at_field("appearance.cursor_size"),
                );
            }
            if appearance.cursor_theme.is_none() {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "cursor_size requires a cursor_theme",
                    )
                    .at_field("appearance.cursor_size"),
                );
            }
            self.use_capability(Capability::Cursors);
            self.changes.push(DeclaredChange::new(
                Capability::Cursors,
                "write_config",
                "kdeglobals [General] cursorSize".to_owned(),
            ));
        }

        if let Some(look_and_feel) = &appearance.look_and_feel {
            if !is_package_id(&look_and_feel.package) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "look_and_feel.package must be a valid package identifier",
                    )
                    .at_field("appearance.look_and_feel.package"),
                );
            }
            if look_and_feel.components.is_empty() || look_and_feel.components.len() > 6 {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "look_and_feel.components must list 1..6 components",
                    )
                    .at_field("appearance.look_and_feel.components"),
                );
            }
            let mut seen = BTreeSet::new();
            for component in &look_and_feel.components {
                if !allowlist::is_look_and_feel_component(component) {
                    self.push(
                        ValidationIssue::new(
                            ErrorCode::ValueInvalid,
                            format!("unknown look-and-feel component: {component}"),
                        )
                        .at_field("appearance.look_and_feel.components"),
                    );
                } else if !seen.insert(component.as_str()) {
                    self.push(
                        ValidationIssue::new(
                            ErrorCode::DuplicateKey,
                            "a look-and-feel component is declared twice",
                        )
                        .at_field("appearance.look_and_feel.components"),
                    );
                }
            }
            self.use_capability(Capability::LookAndFeel);
            self.changes.push(DeclaredChange::new(
                Capability::LookAndFeel,
                "apply_global_theme",
                format!("look-and-feel:{}", look_and_feel.package),
            ));
        }
    }

    fn check_settings(&mut self) {
        let settings = self.manifest.settings.clone();
        if settings.len() > 128 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack may declare at most 128 [[settings]] entries",
                )
                .at_field("settings"),
            );
        }

        let appearance_targets = appearance_config_targets(self.manifest);
        let mut seen = BTreeSet::new();

        for (index, setting) in settings.iter().enumerate() {
            let location = format!("settings[{index}]");
            let Some(target) = ConfigTarget::parse(&setting.target) else {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ConfigTargetUnsupported,
                        "[[settings]] may only target kdeglobals or kwinrc",
                    )
                    .at_path(&location),
                );
                continue;
            };
            if !is_config_name(&setting.group) || !is_config_name(&setting.key) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a config group or key is not a valid KConfig name",
                    )
                    .at_path(&location),
                );
                continue;
            }
            // Duplicate and cross-section checks run before the allowlist checks: a
            // settings row that collides with another declaration is worth reporting even
            // when the key is also engine-owned, and validation aggregates every problem.
            if !seen.insert((
                setting.target.clone(),
                setting.group.clone(),
                setting.key.clone(),
            )) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DuplicateKey,
                        "the same (target, group, key) is declared more than once",
                    )
                    .at_path(&location),
                );
            }
            if appearance_targets.contains(&(
                setting.target.clone(),
                setting.group.clone(),
                setting.key.clone(),
            )) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DuplicateTarget,
                        "[[settings]] and [appearance] set the same target key",
                    )
                    .at_path(&location),
                );
            }
            if allowlist::is_engine_owned_config(target, &setting.group, &setting.key) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ConfigKeyNotAllowed,
                        "an appearance-managed key cannot be set through [[settings]]",
                    )
                    .at_path(&location),
                );
                continue;
            }
            let Some(expected) = allowlist::config_key_type(target, &setting.group, &setting.key)
            else {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ConfigKeyNotAllowed,
                        "the config key is not on the engine allowlist",
                    )
                    .at_path(&location),
                );
                continue;
            };
            if !value_matches(&setting.value, expected) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        format!("the value must be a {}", expected.label()),
                    )
                    .at_path(&location),
                );
            }

            let owners = allowlist::config_key_capabilities(target, &setting.group, &setting.key);
            let mut claimed = false;
            for owner in owners {
                if self.declared.contains(owner) {
                    self.used.insert(*owner);
                    claimed = true;
                }
            }
            if !claimed && let Some(owner) = owners.first() {
                self.use_capability(*owner);
            }

            self.changes.push(DeclaredChange::new(
                owners.first().copied().unwrap_or(Capability::KdeGlobal),
                "write_config",
                format!("{} [{}] {}", setting.target, setting.group, setting.key),
            ));
        }
    }

    fn check_effects(&mut self) {
        let effects = self
            .manifest
            .kwin
            .as_ref()
            .map(|kwin| kwin.effects.clone())
            .unwrap_or_default();
        if effects.len() > 32 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack may declare at most 32 KWin effects",
                )
                .at_field("kwin.effects"),
            );
        }
        let mut seen = BTreeSet::new();
        for effect in &effects {
            if !allowlist::is_kwin_effect(&effect.id) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::EffectNotAllowed,
                        format!("KWin effect is not allowlisted: {}", effect.id),
                    )
                    .at_field("kwin.effects"),
                );
            }
            if !seen.insert(effect.id.as_str()) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DuplicateKey,
                        "a KWin effect is declared more than once",
                    )
                    .at_field("kwin.effects"),
                );
            }
            self.use_capability(Capability::KwinEffects);
            self.changes.push(DeclaredChange::new(
                Capability::KwinEffects,
                "set_kwin_effect",
                format!("kwinrc [Plugins] {}Enabled = {}", effect.id, effect.enabled),
            ));
        }
    }

    fn check_shortcuts(&mut self) {
        let shortcuts = self.manifest.shortcuts.clone();
        if shortcuts.len() > 64 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack may declare at most 64 shortcuts",
                )
                .at_field("shortcuts"),
            );
        }
        let mut seen = BTreeSet::new();
        for shortcut in &shortcuts {
            if !allowlist::is_shortcut_allowed(&shortcut.component, &shortcut.action)
                || !allowlist::is_valid_shortcut_keys(&shortcut.keys)
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ShortcutNotAllowed,
                        "the shortcut component, action, or chord is not allowlisted",
                    )
                    .at_field("shortcuts"),
                );
            }
            if !seen.insert((shortcut.component.as_str(), shortcut.action.as_str())) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DuplicateKey,
                        "a shortcut action is declared more than once",
                    )
                    .at_field("shortcuts"),
                );
            }
            self.use_capability(Capability::Shortcuts);
            self.changes.push(DeclaredChange::new(
                Capability::Shortcuts,
                "write_shortcut",
                format!("{} / {}", shortcut.component, shortcut.action),
            ));
        }
    }

    fn check_panel(&mut self) {
        let Some(panel) = self.manifest.panel.clone() else {
            return;
        };
        if panel.create {
            self.use_capability(Capability::PanelCreate);
            self.changes.push(DeclaredChange::new(
                Capability::PanelCreate,
                "create_panel",
                "new panel".to_owned(),
            ));
        }
        if !panel.properties.is_empty() {
            self.use_capability(Capability::PanelProperties);
        }
        if !panel.widgets.is_empty() {
            self.use_capability(Capability::PanelWidgets);
        }
        if !panel.create && panel.properties.is_empty() && panel.widgets.is_empty() {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ValueInvalid,
                    "[panel] must create a panel or declare a change",
                )
                .at_field("panel"),
            );
        }

        for property in &panel.properties {
            if property.is_empty() {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a [[panel.properties]] row changes nothing",
                    )
                    .at_field("panel.properties"),
                );
            }
            if let Some(location) = &property.location
                && !allowlist::PANEL_LOCATIONS.contains(&location.as_str())
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "panel location must be top, bottom, left, or right",
                    )
                    .at_field("panel.properties"),
                );
            }
            if let Some(height) = property.height
                && !(16..=128).contains(&height)
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "panel height must be between 16 and 128",
                    )
                    .at_field("panel.properties"),
                );
            }
            if let Some(alignment) = &property.alignment
                && !allowlist::PANEL_ALIGNMENTS.contains(&alignment.as_str())
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "panel alignment must be left, center, or right",
                    )
                    .at_field("panel.properties"),
                );
            }
            if let Some(hiding) = &property.hiding
                && !allowlist::PANEL_HIDING.contains(&hiding.as_str())
            {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "panel hiding must be none, autohide, or dodgewindows",
                    )
                    .at_field("panel.properties"),
                );
            }
            self.changes.push(DeclaredChange::new(
                Capability::PanelProperties,
                "set_panel_property",
                "panel properties".to_owned(),
            ));
        }

        for widget in &panel.widgets {
            if widget.panel < 0 {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a panel widget index must not be negative",
                    )
                    .at_field("panel.widgets"),
                );
            }
            if !allowlist::PANEL_WIDGET_OPS.contains(&widget.op.as_str()) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a panel widget operation must be add, configure, or remove",
                    )
                    .at_field("panel.widgets"),
                );
            }
            if !allowlist::is_widget_type_allowed(&widget.widget_type) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        format!("widget type is not allowlisted: {}", widget.widget_type),
                    )
                    .at_field("panel.widgets"),
                );
            }
            if !widget.safe {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "panel.widgets must declare safe = true",
                    )
                    .at_field("panel.widgets"),
                );
            }
            self.changes.push(DeclaredChange::new(
                Capability::PanelWidgets,
                "manage_widget",
                format!("{} {}", widget.op, widget.widget_type),
            ));
        }
    }

    fn check_desktop_behavior(&mut self) {
        let Some(behavior) = self.manifest.desktop_behavior.clone() else {
            return;
        };
        for desktop in &behavior.virtual_desktops {
            if !allowlist::VIRTUAL_DESKTOP_OPS.contains(&desktop.op.as_str()) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ValueInvalid,
                        "a virtual-desktop operation must be add, rename, or remove",
                    )
                    .at_field("desktop_behavior.virtual_desktops"),
                );
            }
            if matches!(desktop.op.as_str(), "add" | "rename") {
                let name = desktop.name.as_deref().unwrap_or("");
                if name.trim().is_empty() || name.chars().count() > 64 {
                    self.push(
                        ValidationIssue::new(
                            ErrorCode::ValueInvalid,
                            "a virtual desktop needs a non-empty name of at most 64 characters",
                        )
                        .at_field("desktop_behavior.virtual_desktops"),
                    );
                }
            }
            self.use_capability(Capability::DesktopBehavior);
            self.changes.push(DeclaredChange::new(
                Capability::DesktopBehavior,
                "manage_virtual_desktop",
                format!("{} {}", desktop.op, desktop.name.as_deref().unwrap_or("")),
            ));
        }
    }

    fn check_compatibility(&mut self) {
        let compatibility = self.manifest.compatibility.clone();
        if compatibility.desktop != "kde" {
            self.push(
                ValidationIssue::new(
                    ErrorCode::CompatibilityInvalid,
                    "schema v1 packs must declare desktop = \"kde\"",
                )
                .at_field("compatibility.desktop"),
            );
        }
        if let Err(error) = PlasmaConstraint::parse(&compatibility.plasma) {
            let mut issue = ValidationIssue::new(ErrorCode::CompatibilityInvalid, error.message);
            issue.field = Some("compatibility.plasma".to_owned());
            self.push(issue);
        }
    }

    fn check_dependencies(&mut self) {
        let Some(dependencies) = self.manifest.dependencies.clone() else {
            return;
        };
        if dependencies.packs.len() > 16 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::LimitExceeded,
                    "a pack may declare at most 16 dependencies",
                )
                .at_field("dependencies.packs"),
            );
        }
        let mut seen = BTreeSet::new();
        for dependency in &dependencies.packs {
            let id = match PackId::parse(&dependency.id) {
                Ok(id) => id,
                Err(_) => {
                    self.push(
                        ValidationIssue::new(
                            ErrorCode::DependencyInvalid,
                            format!("dependency pack ID is invalid: {}", dependency.id),
                        )
                        .at_field("dependencies.packs"),
                    );
                    continue;
                }
            };
            if id == self.manifest.pack.id {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DependencyCycle,
                        "a pack cannot depend on itself",
                    )
                    .at_field("dependencies.packs"),
                );
            }
            if VersionReq::parse(&dependency.version).is_err() {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DependencyInvalid,
                        format!(
                            "dependency version range is invalid: {}",
                            dependency.version
                        ),
                    )
                    .at_field("dependencies.packs"),
                );
            }
            if !seen.insert(id.to_string()) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::DependencyInvalid,
                        "a dependency is declared more than once",
                    )
                    .at_field("dependencies.packs"),
                );
            }
        }
    }

    fn check_checksums(&mut self) {
        let Some(integrity) = self.manifest.integrity.clone() else {
            return;
        };
        if integrity.algorithm != "sha256" {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ChecksumMalformed,
                    "the only supported integrity algorithm is sha256",
                )
                .at_field("integrity.algorithm"),
            );
        }
        if integrity.manifest != CHECKSUM_FILE {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ChecksumMalformed,
                    "integrity.manifest must be checksums.sha256",
                )
                .at_field("integrity.manifest"),
            );
        }
        let Some(entry) = self.tree.entry(CHECKSUM_FILE) else {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ChecksumIncomplete,
                    "the manifest declares checksums.sha256 but the file is missing",
                )
                .at_path(CHECKSUM_FILE),
            );
            return;
        };
        if entry.kind != EntryKind::File || entry.size == 0 {
            self.push(
                ValidationIssue::new(
                    ErrorCode::ChecksumMalformed,
                    "checksums.sha256 must be a non-empty file",
                )
                .at_path(CHECKSUM_FILE),
            );
            return;
        }

        let path = self.root.join(CHECKSUM_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let mut issue = ValidationIssue::new(
                    error_code_for(&error),
                    "the checksum file could not be read",
                );
                issue.path = Some(CHECKSUM_FILE.to_owned());
                self.push(issue);
                return;
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::ChecksumMalformed,
                        "checksums.sha256 is not valid UTF-8",
                    )
                    .at_path(CHECKSUM_FILE),
                );
                return;
            }
        };

        match integrity::parse(text) {
            Ok(checksums) => {
                for issue in integrity::verify(&checksums, self.tree) {
                    let mut reported = ValidationIssue::new(issue.code, issue.message);
                    reported.path = Some(CHECKSUM_FILE.to_owned());
                    self.push(reported);
                }
                self.integrity_source = IntegritySource::Declared;
            }
            Err(issues) => {
                for issue in issues {
                    let mut reported = ValidationIssue::new(issue.code, issue.message);
                    reported.path = Some(CHECKSUM_FILE.to_owned());
                    self.push(reported);
                }
            }
        }
    }

    fn check_references(&mut self) {
        for entry in &self.tree.entries {
            if entry.kind != EntryKind::File {
                continue;
            }
            if !self.referenced.contains(&entry.relative) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::UnreferencedFile,
                        "the file is not referenced by the manifest, a preview, a document, or checksums",
                    )
                    .at_path(&entry.relative),
                );
            }
        }
    }

    fn check_capability_usage(&mut self) {
        let declared = self.declared.clone();
        for capability in &declared {
            if !self.used.contains(capability) {
                self.push(
                    ValidationIssue::new(
                        ErrorCode::CapabilityUnused,
                        format!("the pack requests a capability but declares nothing for it: {capability}"),
                    )
                    .at_field("capabilities.requires"),
                );
            }
        }
    }
}

fn capability_for_asset(kind: AssetKind) -> Capability {
    match kind {
        AssetKind::ColorScheme => Capability::ColorScheme,
        AssetKind::IconTheme => Capability::Icons,
        AssetKind::CursorTheme => Capability::Cursors,
        AssetKind::PlasmaStyle => Capability::PlasmaStyle,
        AssetKind::WindowDecoration => Capability::WindowDecorations,
        AssetKind::Wallpaper => Capability::Wallpaper,
    }
}

fn path_matches_kind(kind: AssetKind, path: &str) -> bool {
    match kind {
        AssetKind::ColorScheme | AssetKind::WindowDecoration => is_under(path, "themes/"),
        AssetKind::IconTheme => is_under(path, "icons/"),
        AssetKind::CursorTheme => is_under(path, "cursors/"),
        AssetKind::PlasmaStyle => is_under(path, "plasma/"),
        AssetKind::Wallpaper => is_under(path, "wallpapers/"),
    }
}

fn appearance_config_targets(manifest: &Manifest) -> BTreeSet<(String, String, String)> {
    let mut targets = BTreeSet::new();
    let Some(appearance) = &manifest.appearance else {
        return targets;
    };
    let mut add = |target: &str, group: &str, key: &str| {
        targets.insert((target.to_owned(), group.to_owned(), key.to_owned()));
    };
    if appearance.color_scheme.is_some() {
        add("kdeglobals", "General", "ColorScheme");
    }
    if appearance.cursor_theme.is_some() {
        add("kdeglobals", "General", "cursorTheme");
    }
    if appearance.cursor_size.is_some() {
        add("kdeglobals", "General", "cursorSize");
    }
    if appearance.icon_theme.is_some() {
        add("kdeglobals", "Icons", "Theme");
    }
    if appearance.window_decoration.is_some() {
        add("kdeglobals", "org.kde.kdecoration2", "theme");
        add("kdeglobals", "org.kde.kdecoration2", "library");
    }
    if appearance.plasma_style.is_some() {
        add("plasmarc", "Theme", "name");
    }
    targets
}

fn is_config_name(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphanumeric() => characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '_' | '.' | '-')
        }),
        _ => false,
    }
}

fn is_spdx_expression(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return false;
    }
    let mut has_identifier = false;
    for token in value.split_whitespace() {
        let token = token.trim_matches(['(', ')']);
        if token.is_empty() {
            continue;
        }
        if matches!(token, "AND" | "OR" | "WITH") {
            continue;
        }
        if !token.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
        }) {
            return false;
        }
        has_identifier = true;
    }
    has_identifier
}

fn is_package_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
        && !value.starts_with('.')
        && !value.ends_with('.')
}

fn value_matches(value: &SettingValue, expected: ConfigValueType) -> bool {
    match (value, expected) {
        (SettingValue::Boolean(_), ConfigValueType::Boolean)
        | (SettingValue::Integer(_), ConfigValueType::Integer) => true,
        (SettingValue::String(text), ConfigValueType::String) => text.len() <= 4096,
        (SettingValue::Strings(items), ConfigValueType::StringArray) => {
            items.len() <= 64 && items.iter().all(|item| item.len() <= 1024)
        }
        _ => false,
    }
}

fn error_code_for(error: &std::io::Error) -> ErrorCode {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ErrorCode::IoPermission
    } else {
        ErrorCode::Io
    }
}
