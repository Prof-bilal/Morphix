//! Plasma compatibility range parsing and evaluation.
//!
//! **Ownership.** Parses the `[compatibility].plasma` expression from a pack manifest and
//! evaluates it against a detected [`PlasmaVersion`]. Declarative only: no I/O.
//!
//! # v1 rules (PACK_SPEC.md §5.3)
//!
//! A schema-v1 pack range must not admit any version below `6.0.0`, and must admit at
//! least one version below `7.0.0`. Those rules are enforced at parse time, so the planner
//! cannot accidentally accept a pack that would run on Plasma 5 or that matches nothing.
//!
//! The evaluation is deliberately conservative. The expression is a conjunction of
//! `semver` comparators (the crate's grammar has no `||`); the lowest admitted version is
//! derived from the comparators' lower bounds. A witness below `7.0.0` must actually
//! match; this rejects inverted or empty ranges such as `>=6.0, <6.0` even though their
//! syntax is valid.
//!
//! # Pre-releases
//!
//! Matching uses [`PlasmaVersion::release`] (the release triple), not the pre-release
//! suffix. `semver`'s range matching would otherwise reject every pre-release unless the
//! pack declared a pre-release comparator, which v1 packs do not do. The full version is
//! still displayed; a pre-release session is outside the validated matrix anyway.

use std::fmt;

use semver::{BuildMetadata, Comparator, Op, Prerelease, Version, VersionReq};

use crate::core::versions::PlasmaVersion;
use crate::errors::{ErrorCode, MorphixError, Result};

/// The lower bound every v1 pack range must respect.
const PLASMA_6: Version = Version::new(6, 0, 0);

/// The exclusive upper bound every v1 pack range must admit a version below.
const PLASMA_7: Version = Version::new(7, 0, 0);

/// A validated v1 Plasma compatibility range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlasmaConstraint {
    expression: String,
    requirement: VersionReq,
}

impl PlasmaConstraint {
    /// Parses and validates a `[compatibility].plasma` expression.
    ///
    /// # Errors
    ///
    /// Returns `E_COMPATIBILITY_INVALID` when the expression is empty or malformed, has no
    /// lower bound, admits a version below `6.0.0`, admits no version below `7.0.0`, or is
    /// unsatisfiable. Context keys are `expression` and `reason`.
    pub fn parse(expression: &str) -> Result<Self> {
        let expression = expression.trim();
        if expression.is_empty() {
            return Err(invalid_constraint(expression, "the range is empty"));
        }

        let requirement = VersionReq::parse(expression)
            .map_err(|_| invalid_constraint(expression, "the range syntax is invalid"))?;

        let Some(lower) = lowest_admitted(&requirement.comparators) else {
            return Err(invalid_constraint(
                expression,
                "the range has no lower bound and would admit Plasma 5",
            ));
        };

        if lower < PLASMA_6 {
            return Err(invalid_constraint(
                expression,
                "the range admits versions below 6.0.0",
            ));
        }
        if lower >= PLASMA_7 {
            return Err(invalid_constraint(
                expression,
                "the range admits no version below 7.0.0",
            ));
        }
        if !has_witness_below_seven(&requirement, &lower) {
            return Err(invalid_constraint(
                expression,
                "the range is inverted or admits no version below 7.0.0",
            ));
        }

        Ok(Self {
            expression: expression.to_owned(),
            requirement,
        })
    }

    /// Evaluates the range against a detected Plasma version.
    ///
    /// The pre-release suffix is ignored; see the module docs.
    #[must_use]
    pub fn matches(&self, version: &PlasmaVersion) -> bool {
        self.requirement.matches(&version.release())
    }

    /// The original expression, trimmed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.expression
    }
}

impl fmt::Display for PlasmaConstraint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.expression)
    }
}

/// The greatest lower bound implied by the comparators, if any.
fn lowest_admitted(comparators: &[Comparator]) -> Option<Version> {
    let mut lowest: Option<Version> = None;
    for comparator in comparators {
        let Some(bound) = lower_bound(comparator) else {
            continue;
        };
        lowest = Some(match lowest {
            Some(current) if current >= bound => current,
            _ => bound,
        });
    }
    lowest
}

/// The version at which a comparator starts admitting versions.
///
/// Partial `>` comparators advance the next component, matching the `semver` grammar
/// (`>6.3` is `>=6.4.0`, not `>6.3.0`).
fn lower_bound(comparator: &Comparator) -> Option<Version> {
    let patch = comparator.patch.unwrap_or(0);
    let minor = comparator.minor.unwrap_or(0);

    match comparator.op {
        Op::Exact | Op::GreaterEq | Op::Tilde | Op::Caret => Some(version_with_pre(
            comparator.major,
            minor,
            patch,
            &comparator.pre,
        )),
        Op::Greater => match (comparator.minor, comparator.patch) {
            (None, _) => Some(Version::new(comparator.major.saturating_add(1), 0, 0)),
            (Some(_), None) => Some(Version::new(comparator.major, minor.saturating_add(1), 0)),
            (Some(_), Some(_)) => Some(version_with_pre(
                comparator.major,
                minor,
                patch,
                &comparator.pre,
            )),
        },
        Op::Less | Op::LessEq | Op::Wildcard => None,
        // `Op` is `#[non_exhaustive]`. A future operator cannot be assumed to impose a
        // lower bound; treating it as none makes the range fail the lower-bound check.
        _ => None,
    }
}

fn version_with_pre(major: u64, minor: u64, patch: u64, pre: &Prerelease) -> Version {
    Version {
        major,
        minor,
        patch,
        pre: pre.clone(),
        build: BuildMetadata::EMPTY,
    }
}

/// Proves the range is satisfiable and has at least one admitted version below 7.0.0.
///
/// The candidate set is derived from the lowest bound and the common Plasma release
/// lines. Every candidate must actually match, so this never weakens the lower-bound
/// check; it only rejects empty or inverted ranges that pass the syntax check.
fn has_witness_below_seven(requirement: &VersionReq, lower: &Version) -> bool {
    let mut candidates = vec![
        lower.clone(),
        next_patch(lower),
        Version::new(6, 0, 0),
        Version::new(6, 0, 1),
        Version::new(6, 1, 0),
        Version::new(6, 3, 0),
        Version::new(6, 3, 5),
        Version::new(6, 4, 0),
        Version::new(6, 9, 9),
        Version::new(6, 10, 0),
        Version::new(6, 99, 99),
    ];
    // A range whose lower bound is a 7.0.0 pre-release can still admit pre-releases below
    // the release; test one explicitly.
    if let Ok(pre_release) = Version::parse("7.0.0-alpha.1") {
        candidates.push(pre_release);
    }

    candidates
        .into_iter()
        .any(|candidate| candidate < PLASMA_7 && requirement.matches(&candidate))
}

fn next_patch(version: &Version) -> Version {
    Version::new(
        version.major,
        version.minor,
        version.patch.saturating_add(1),
    )
}

fn invalid_constraint(expression: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(
        ErrorCode::CompatibilityInvalid,
        "the Plasma compatibility range is not usable for a v1 pack",
    )
    .with_context("expression", expression)
    .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::PlasmaConstraint;
    use crate::core::versions::PlasmaVersion;
    use crate::errors::ErrorCode;

    fn version(text: &str) -> PlasmaVersion {
        PlasmaVersion::parse(text).expect("version parses")
    }

    fn reason(error: &crate::errors::MorphixError) -> String {
        error
            .context
            .iter()
            .find(|entry| entry.key == "reason")
            .map(|entry| entry.value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn accepts_the_documented_v1_range() {
        let constraint = PlasmaConstraint::parse(">=6.0, <7.0").expect("valid range");

        assert!(constraint.matches(&version("6.0.0")));
        assert!(constraint.matches(&version("6.3.5")));
        assert!(constraint.matches(&version("6.10.1")));
        assert!(!constraint.matches(&version("5.27.11")));
        assert!(!constraint.matches(&version("7.0.0")));
        assert_eq!(constraint.as_str(), ">=6.0, <7.0");
        assert_eq!(constraint.to_string(), ">=6.0, <7.0");
    }

    #[test]
    fn accepts_caret_tilde_and_exact_ranges() {
        for (expression, inside, outside) in [
            ("^6.3", "6.3.0", "6.2.9"),
            ("~6.3", "6.3.5", "6.4.0"),
            ("=6.3.5", "6.3.5", "6.3.6"),
            (">=6", "6.0.0", "5.99.99"),
            (">6.0.0", "6.0.1", "6.0.0"),
        ] {
            let constraint = PlasmaConstraint::parse(expression).expect("valid range");
            assert!(
                constraint.matches(&version(inside)),
                "{expression} should match {inside}"
            );
            assert!(
                !constraint.matches(&version(outside)),
                "{expression} should not match {outside}"
            );
        }
    }

    #[test]
    fn a_partial_greater_range_starts_at_the_next_component() {
        // `>6.3` is `>=6.4.0`; `>6` is `>=7.0.0` and must be rejected as not admitting 6.x.
        let constraint = PlasmaConstraint::parse(">6.3, <7.0").expect("valid range");
        assert!(!constraint.matches(&version("6.3.9")));
        assert!(constraint.matches(&version("6.4.0")));

        let error = PlasmaConstraint::parse(">6").expect_err("no 6.x version is admitted");
        assert_eq!(error.code, ErrorCode::CompatibilityInvalid);
    }

    #[test]
    fn wildcard_empty_and_missing_lower_bounds_are_refused() {
        for expression in ["", "   ", "*", "<7.0", "<=6.9"] {
            let error = PlasmaConstraint::parse(expression).expect_err("must be refused");
            assert_eq!(
                error.code,
                ErrorCode::CompatibilityInvalid,
                "expression: {expression:?}"
            );
            assert!(!reason(&error).is_empty());
        }
    }

    #[test]
    fn ranges_that_admit_plasma_5_are_refused() {
        for expression in [">=5.0", ">=5.27, <7.0", "=5.27.11", "^5.27", ">5.0"] {
            let error = PlasmaConstraint::parse(expression).expect_err("Plasma 5 range");
            assert_eq!(error.code, ErrorCode::CompatibilityInvalid);
            assert!(
                reason(&error).contains("below 6.0.0"),
                "expression {expression}: {}",
                reason(&error)
            );
        }
    }

    #[test]
    fn ranges_without_a_supported_6_x_version_are_refused() {
        for expression in [">=7.0", ">=7.0, <8.0", ">=6.0, >=7.0"] {
            let error = PlasmaConstraint::parse(expression).expect_err("no 6.x version");
            assert_eq!(error.code, ErrorCode::CompatibilityInvalid);
            assert!(
                reason(&error).contains("no version below 7.0.0"),
                "expression {expression}: {}",
                reason(&error)
            );
        }
    }

    #[test]
    fn inverted_or_empty_ranges_are_refused() {
        for expression in [">=6.0, <6.0", ">=6.0, <=5.9", ">=7.0, <6.0"] {
            let error = PlasmaConstraint::parse(expression).expect_err("empty range");
            assert_eq!(error.code, ErrorCode::CompatibilityInvalid);
        }
    }

    #[test]
    fn malformed_syntax_is_refused() {
        for expression in ["garbage", ">=x", "6..3", ">=6.0,", "~", "^", ">=6.0 <7.0"] {
            let error = PlasmaConstraint::parse(expression).expect_err("malformed");
            assert_eq!(
                error.code,
                ErrorCode::CompatibilityInvalid,
                "expression: {expression:?}"
            );
        }
    }

    #[test]
    fn pre_release_runtime_versions_match_their_release_line() {
        let constraint = PlasmaConstraint::parse(">=6.0, <7.0").expect("valid range");

        assert!(constraint.matches(&version("6.4.0-rc1")));
        assert!(constraint.matches(&version("6.4.0-rc.2+build.7")));
        assert!(!constraint.matches(&version("5.27.11-rc1")));
    }
}
