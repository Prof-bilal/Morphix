# Morphix — MVP Definition (V0.1)

- **Status:** Design baseline. V0.1 is not implemented.
- **Scope:** KDE Plasma 6 only (`>=6.0, <7.0`). No other desktop environment at any point
  in this milestone.
- **Companion documents:** [PRD.md](PRD.md) (why), [ARCHITECTURE.md](ARCHITECTURE.md)
  (how), [PACK_SPEC.md](PACK_SPEC.md) (pack format), [TRANSACTIONS.md](TRANSACTIONS.md)
  (mutation model), [TESTING.md](TESTING.md) (how it is proven).

V0.1 exists to prove one claim: **a desktop customization can be previewed, applied, and
fully reversed without the user editing files or trusting a shell script.** Everything in
this document is scoped to that claim.

---

## 1. Supported environment gate

A system is **Supported** only if all of the following hold:

| Condition | Detection | Failure behavior |
| --- | --- | --- |
| Linux (`/proc` present, `/etc/os-release` readable) | read-only filesystem probe | `Unsupported` with reason `NOT_LINUX` |
| Plasma 6.x running in the current user session (`>=6.0, <7.0`) | DBus `org.kde.plasmashell` presence + version probe | `Unsupported` with reason `NO_PLASMA` / `PLASMA_VERSION_UNSUPPORTED` |
| Session bus reachable | DBus connect + ping | `Unsupported` with reason `NO_SESSION_BUS` |
| Morphix data dir writable | create-and-delete a probe file in `~/.local/share/morphix/` | `Unsupported` with reason `DATA_DIR_UNWRITABLE` |
| Required KDE interfaces respond | adapter probe (`kdeglobals` readable, `kwinrc` readable, `kglobalaccel` present) | `Unsupported` with per-interface reasons |
| Not running as root (UID != 0) | `geteuid()` | `Unsupported` with reason `RUNNING_AS_ROOT` |

Everything else — distribution family, package manager, Wayland vs X11, language, screen
count — is **informational**. The distribution is never a hard gate in V0.1; releases list
validated distributions and the UI shows a "validated / not yet validated" note without
blocking apply on an unvalidated distro.

Unsupported systems: catalog browsing and pack inspection remain available (read-only);
planning and apply are refused with the specific reason.

## 2. Must Have (V0.1)

Each item corresponds to an acceptance-criteria group in §7.

### 2.1 System awareness

- Detect distribution from `/etc/os-release` (ID, NAME, VERSION_ID, PRETTY_NAME), with
  tolerant parsing.
- Detect KDE Plasma version, session type (Wayland/X11), and plasmashell liveness.
- Detect package manager from read-only package database files; informational only.
- Compute and display the support verdict with reasons and remediation hints.

### 2.2 Catalog and pack inspection

- Browse the four bundled packs plus locally installed packs.
- Show pack name, version, summary, preview images, license/attribution, compatibility,
  declared capabilities, and asset inventory.
- Show the *declared* change set before planning: which asset kinds, which allowlisted
  settings, which shortcuts, which KWin effects.
- Install a local pack from a directory, with full validation, into the Morphix pack
  store. No execution of pack content, ever.

### 2.3 Preview

- Generate a deterministic change plan with: resolved targets, installed asset sizes and
  destination paths, config key old → new values, shortcut old → new values, KWin effect
  state changes, requested capabilities, warnings, and restart requirements.
- Preview performs **zero** mutations: no writes to KDE config, no writes to XDG theme
  locations, no DBus state changes.
- Refuse plans for unsupported systems, incompatible packs, invalid manifests, capability
  mismatches, target collisions, or unsupported resources.

### 2.4 Snapshot and apply

- Snapshot every affected resource before the first mutation; verify the snapshot is
  readable before proceeding.
- Apply all changes through the transaction engine, one exclusive mutation at a time.
- Journal every operation state transition; support crash recovery on next launch.
- Verify post-state; commit only after verification.
- On any failure, reverse completed operations in reverse order, verify restoration, and
  report per-operation outcomes.

### 2.5 Reversibility and history

- Roll back any committed Morphix transaction, restoring the exact pre-transaction state
  for resources Morphix touched.
- Refuse rollback that would overwrite changes made by a later transaction, unless the
  user explicitly overrides with a clearly labeled confirmation.
- Restore any stored snapshot through the same plan → apply → verify path.
- Show local history of transactions, operations, snapshots, and outcomes.

### 2.6 Permissions and refusal

- Closed capability set (§4); packs request a subset.
- Capability confirmation bound to the plan hash before apply.
- Refuse unsupported or unsafe operations with a stable error code and an explanation of
  what was not done and whether anything was changed.
- No root, no `sudo`, no privileged helper, no network, no telemetry.

### 2.7 Bundled demo packs

Four declarative packs, original/open assets only, no proprietary brand assets:

| Pack | Intent | Notes for V0.1 |
| --- | --- | --- |
| Cupertino | macOS-inspired light and dark composition | Original icons, cursors, wallpaper, colors; traffic-light controls are not reproduced by Morphix (the Aurorae decoration is original) |
| Windows Inspired | Modern Windows-inspired layout | Original equivalents; no Microsoft logos, fonts, or wallpapers; panel changes are guidance only |
| Minimal Developer | Low-noise interface and developer shortcuts | Subdued colors, compact fonts where safe, developer-oriented shortcuts |
| Glass | Transparency, blur, floating-style panels | Floating panels cannot be automated in V0.1; shipped as documented manual steps, while blur/transparency/colors/wallpaper are applied |

Each pack must plan and apply on a clean Plasma 6 install, and roll back to the exact
pre-pack state.

## 3. Should Have Later (post-V0.1 candidates)

Ordered by expected value, not committed to a release. Items that must never enter V0.1,
including the current release's hard non-goals, are listed in §10.

- Panel properties (floating, thickness, alignment) and Plasma layout replacement.
- Pack uninstall for packs that are installed but never applied.
- Snapshot tagging, retention policy, and manual snapshots.
- Pack updates with diffed upgrade plans.
- Pack signing and a local trust store.
- Optional read-only registry with user-initiated downloads.
- CLI front end over `morphix-core`.
- GNOME adapter (requires ADR and adapter-contract revision).
- Profile export/import as an ordered transaction set.
- Localization.
- Drag-and-drop pack installation.

## 4. Capability set (V0.1)

Nine capabilities, closed set. Each maps to a fixed target table in
[ARCHITECTURE.md](ARCHITECTURE.md) §6. Packs cannot invent capabilities or paths.

| Capability | What it allows | Targets |
| --- | --- | --- |
| `appearance.color_scheme` | Install a `.colors` file and select it | `~/.local/share/color-schemes/`, `kdeglobals` |
| `appearance.icons` | Install and select an icon theme | `~/.local/share/icons/`, `kdeglobals` |
| `appearance.cursors` | Install and select a cursor theme and size | `~/.local/share/icons/`, `kdeglobals` |
| `appearance.plasma_style` | Install and select a Plasma style | `~/.local/share/plasma/desktoptheme/`, `plasmarc` |
| `appearance.window_decorations` | Install and select an Aurorae decoration | `~/.local/share/aurorae/themes/`, `kdeglobals` |
| `appearance.wallpaper` | Install a wallpaper and set the current desktop background | `~/.local/share/morphix/assets/wallpapers/`, Plasma DBus |
| `settings.kde_global` | Write allowlisted `kdeglobals` keys | `~/.config/kdeglobals` |
| `settings.kwin_effects` | Toggle/configure allowlisted KWin effects | `~/.config/kwinrc`, KWin reconfigure |
| `settings.shortcuts` | Set allowlisted global shortcuts | `~/.config/kglobalshortcutsrc`, `kglobalaccel` DBus |

Anything not in this table is out of scope and must be rejected at validation or planning
time. Panel configuration and Plasma layouts are intentionally absent (ADR-011).

## 5. Resource scope

| Resource | V0.1 status | Mechanism |
| --- | --- | --- |
| Color scheme | Supported | Asset install + `kdeglobals` keys |
| Icon theme | Supported | Asset install + `kdeglobals` keys |
| Cursor theme | Supported | Asset install + `kdeglobals` keys (theme and size) |
| Plasma style | Supported | Asset install + `kdeglobals` keys |
| Window decorations | Supported (Aurorae only) | Asset install + `kdeglobals` keys |
| Wallpaper | Supported with gate | Asset install + constrained Plasma scripting call (ADR-015) |
| KDE global settings | Supported, allowlisted keys only | `kdeglobals` |
| KWin configuration | Supported, allowlisted effects/keys only | `kwinrc` + reconfigure |
| Keyboard shortcuts | Supported, allowlisted components/actions | `kglobalshortcutsrc` + `kglobalaccel` |
| Panel configuration | Not V0.1 | Guidance text only |
| Plasma layout | Not V0.1 | Guidance text only |
| Arbitrary files | Never | Rejected |
| Packages, services, drivers | Never | Rejected |

## 6. Non-functional MVP requirements

- Offline: zero network calls; no auto-update check.
- No root: refuses to start mutations when `geteuid() == 0`.
- Single mutation at a time, enforced by an exclusive file lock plus a DB transaction.
- Plan generation for a bundled pack: < 2 s on a modern laptop, measured on the Phase 10
  benchmark corpus.
- Apply overhead beyond asset copy: < 1 s for a typical pack.
- No panic on hostile pack input; all pack-facing code returns typed errors.
- Logs never contain file contents or full asset blobs.

## 7. Acceptance criteria

Every criterion is testable by an automated test or a documented manual procedure. IDs are
referenced by [TASKS.md](TASKS.md) and [TESTING.md](TESTING.md).

### AC-01 Distribution detection
Given a fixture `/etc/os-release`, the detector returns ID, name, and version. Given a
missing file, it returns `None` for distro fields without error. Given a malformed line, it
skips that line and parses the rest. Covered by unit tests over at least 5 real distro
fixtures (Arch, Fedora, Ubuntu, openSUSE, Debian).

### AC-02 Plasma detection
On a Plasma 6 session, the detector reports `plasma_version >= 6.0 < 7.0`, session type,
and `plasmashell_running = true`. On a fixture with a 5.27 version, support verdict is
`Unsupported` with `PLASMA_VERSION_UNSUPPORTED`. With no session bus, verdict is
`Unsupported` with `NO_SESSION_BUS`. Parsing tests cover `6.0.0`, `6.3.5`, `6.4.0`, and
malformed strings.

### AC-03 Package manager detection
Detects pacman/dpkg/rpm/zypper from read-only database files without invoking a package
manager. Returns `Unknown` when none is found. Result is displayed but never gates apply.

### AC-04 Support verdict
The status object contains `support.level` (`supported` / `unsupported` / `unverified`),
a non-empty `reasons` array on non-supported verdicts, and a human-readable `summary`.
UI renders the verdict before any pack action. A unit test covers every refusal reason.

### AC-05 Catalog
With the four bundled packs present, the catalog lists exactly four packs with name,
version, summary, preview, compatibility, capabilities, and applied state. Removing a
bundled pack from resources yields a skipped-pack diagnostic, not a crash. Catalog loads
in < 500 ms with bundled packs (cold, excluding UI paint).

### AC-06 Pack detail
The detail view shows description, license and attribution, preview images, asset
inventory, compatibility expression, and the declared change set. A pack with a preview
file missing from disk fails validation at catalog load with a specific error and is shown
as invalid rather than openable.

### AC-07 Compatibility display
Compatibility evaluates against the current Plasma version. `>=6.0` on 6.3.5 is
compatible; `>=6.5` on 6.3.5 is incompatible and apply is disabled with the exact
constraint and detected version shown.

### AC-08 Declared change set
Before planning, the UI lists every declared resource by capability and target kind, with
no hidden fields. A test asserts that the rendered change-set item count equals the
validator's declared-operation count for each bundled pack.

### AC-09 Plan content
A plan for a bundled pack contains: at least one asset operation, the expected config
operations, capability list, warnings array, restart requirement flag, total installed
bytes, and a stable plan hash. Running planning twice on unchanged state yields identical
plan content and identical hash.

### AC-10 Plan purity
During planning, a recording filesystem and adapter wrapper observes zero write, delete,
rename, or DBus-mutating calls. This test runs for every bundled pack.

### AC-11 Snapshot creation
Before the first mutation, a snapshot exists containing an entry per affected resource,
each with a content hash and readable blob (or config value record). Deleting any blob and
re-running apply fails before mutating with `E_SNAPSHOT_UNREADABLE`. Snapshot metadata is
fsynced before apply begins (verified by an injectable journal spy test).

### AC-12 Transactional apply
A successful apply produces: a committed transaction, one `applied` operation record per
planned operation, verified post-state, and an updated applied-pack record. No operation
runs outside the journal. The mutation lock is held for the whole apply.

### AC-13 Failure rollback
With a fault injected at operation N of M for every N, apply fails, all completed
operations are reverted in reverse order, verification of restoration passes, and the
transaction is recorded as `rolled_back` with per-operation outcomes. The UI/report states
that rollback succeeded. This is parameterized over N for every bundled pack.

### AC-14 Rollback of committed transaction
Rolling back a committed transaction restores every resource it touched to its pre-apply
state: config keys and shortcuts return to their previous values (or are removed if they
did not exist), KWin effects and wallpaper return to their previous state, and asset paths
installed by the pack are removed. Unrelated keys and files changed by the user are
preserved. A new committed rollback transaction is recorded and the pack shows as not
applied.

### AC-15 Rollback conflict refusal
If a later committed transaction modified a resource in the rollback scope, rollback
returns `E_ROLLBACK_CONFLICT`, mutates nothing, lists the conflicting resources and the
later transactions, and offers an explicit override path that records the override in
history.

### AC-16 Snapshot restore
Restoring an older snapshot builds and applies a plan that returns the snapshot's
resources to their recorded state, verifies them, and records a transaction linking the
source snapshot. A snapshot whose manifest fails its own hash check is rejected with
`E_SNAPSHOT_CORRUPT`.

### AC-17 History
History lists transactions with kind, pack, start/end time, outcome, and operation count;
detail shows each operation's target, old → new value summary, and result. History
survives an app restart. Rollback and recovery events appear in history.

### AC-18 Local pack install
Installing a valid local pack copies it into the pack store, records its integrity
metadata, and makes it available for planning. Installing an invalid pack (bad TOML,
unsupported schema, path traversal, missing asset, checksum mismatch, unknown capability)
writes nothing and returns field-level errors. Installing a pack with the same ID and
version as an installed pack is refused with `E_PACK_ALREADY_INSTALLED`.

### AC-19 Manifest validation
The validator rejects, with a specific error code: unknown fields, unsupported
`schema_version`, invalid IDs, non-semver versions, incompatible desktop/Plasma
constraints, undeclared capability usage, asset path escaping the pack root, symlinked
assets pointing outside the root, oversized files, unsupported file types, and duplicate
asset keys. Each case has a fixture.

### AC-20 Capability consent
Apply requires consent whose recorded plan hash matches the plan being applied. Applying
without consent returns `E_CONSENT_REQUIRED`. Changing the pack or system state after
planning returns `E_PLAN_STALE` and writes nothing.

### AC-21 Refusal
For each unsupported resource/operation fixture, the planner returns
`E_CAPABILITY_DENIED` or `E_UNSUPPORTED_RESOURCE` and the apply path is never invoked.
The error payload includes what was refused, why, and that no changes were made.

### AC-22 No privilege escalation
The test suite asserts there is no `sudo` invocation, no process spawn with pack-provided
arguments, and no write outside the allowlisted roots. On a `geteuid() == 0` test double,
mutations are refused with `E_RUNNING_AS_ROOT`.

### AC-23 Crash recovery
A transaction interrupted after operation N is detected at startup for every N, and the
pre-transaction state is restored and verified. If a required blob is missing, the app
enters `Recovery required`, lists affected resources, and does not claim restoration.

### AC-24 Wallpaper gate
If the adapter probe reports wallpaper support available, apply sets the wallpaper and
verification reads back the expected image path; rollback restores the previously captured
wallpaper descriptor. If support is unavailable, the plan contains a `manual_step`
warning, does not include a wallpaper mutation, and says so in the UI.

### AC-25 Offline and local-first
An integration run with network access blocked (or an assertion that no network client is
linked into the apply path) completes browse → plan → apply → rollback. No account, token,
or remote endpoint exists in the codebase.

### AC-26 Demo packs
Each of the four bundled packs: validates, plans, applies, and rolls back on a supported
Plasma 6 test VM, leaving no Morphix-installed files and no modified config keys. Previews
render. No shipped asset is proprietary; each pack carries an attribution file.

## 8. MVP validation metrics

Measured locally from the transaction database and exposed in Diagnostics; exported only
when the user chooses. No telemetry.

| Metric | Source | Target |
| --- | --- | --- |
| Install success | launch events with a support verdict / launches | ≥ 95% of reported runs |
| Pack application success | committed applies / apply attempts | ≥ 95% on supported systems |
| Rollback success | verified rollbacks / rollback attempts | 100% automated; ≥ 99% reported |
| Failures by code | `transactions.failure_code` distribution | Every failure has a code; no unknown failures |
| Second-pack usage | distinct packs applied per install within 7 days | ≥ 40% of opted-in reports |
| Additional-pack requests | issue tracker labels | Tracked qualitatively |
| Time to first apply | first commit timestamp − first launch timestamp | median < 3 minutes |
| Recovery outcomes | interrupted transactions resolved without manual edits | ≥ 95% |

Metrics targets are also stated in [PRD.md](PRD.md) §13 and must stay identical.

## 9. Definition of done for V0.1

1. All Must Have items implemented and all AC-01…AC-26 passing in CI (adapter integration
   tests on a Plasma 6 VM may be a separate required job).
2. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, frontend typecheck,
   frontend unit tests, and production build all pass.
3. No TODO in code paths reachable from apply/rollback that would leave state unrecoverable.
4. [SECURITY.md](SECURITY.md) mitigations implemented or explicitly tracked with an
   accepted-risk note in [DECISIONS.md](DECISIONS.md).
5. Documentation matches behavior; any deviation is recorded as an ADR.
6. A fresh VM test: install app, apply each demo pack, roll back each, restore a snapshot,
   crash mid-apply and recover.

## 10. Explicitly not in MVP

Hard exclusions for V0.1. Any of these requires a new ADR before it is even scoped, and
most contradict the product's safety model rather than merely being deferred.

- Other desktop environments (GNOME, Hyprland, XFCE, Cinnamon, MATE, Budgie, COSMIC).
- Plasma 5 support.
- Panel configuration and Plasma layout replacement (ADR-011).
- Plasma global look-and-feel package application (ADR-012).
- Any pack-provided script, hook, binary, interpreter file, or shell command (ADR-006).
- Pack-declared filesystem paths or arbitrary write targets (ADR-010).
- Arbitrary config keys: writes are limited to the documented allowlist.
- System-wide changes, root execution, `sudo`/`pkexec`, daemons, setuid, privileged helpers
  (ADR-016).
- Installing packages, drivers, kernel parameters, services, autostart entries, systemd
  units, or `.desktop` files.
- Panel applets, widgets, KWin scripts, or third-party KWin plugins.
- Keyboard layouts, input methods, locale, timezone, account, or lock-screen settings.
- Environment variables, shell configuration, or `/etc` changes.
- Network access, accounts, cloud sync, telemetry, registry, auto-update, or pack
  monetization (ADR-008).
- Silent uninstall of an applied pack: rollback is the only removal path in V0.1.
- Automatic snapshot pruning or deletion.
- Windows, macOS, mobile, or web builds.
- Multi-user, fleet, or policy management.
