# Morphix — Product Requirements Document

- **Status:** Design baseline (no implementation yet)
- **Product version described:** V1, with the MVP defined as V0.1
- **Desktop scope:** KDE Plasma 6 only
- **Related documents:** [MVP.md](MVP.md), [ARCHITECTURE.md](ARCHITECTURE.md),
  [PACK_SPEC.md](PACK_SPEC.md), [SECURITY.md](SECURITY.md), [DECISIONS.md](DECISIONS.md)

---

## 1. Problem statement

Linux desktop customization is powerful, but it is delivered through mechanisms that
assume the user trusts the author completely.

Today a user who wants a coherent desktop look must:

1. Find a theme or "ricing" guide on a website, forum, or video.
2. Run a shell script or copy directories into undocumented locations.
3. Overwrite KDE configuration keys with no backup.
4. Restart or break Plasma components when something goes wrong.
5. Guess how to undo it, usually by manually restoring files they never recorded.

The consequences are real: lost configuration, half-applied themes, broken keyboard
shortcuts, broken panels, and a culture of "just reinstall" as the recovery strategy.
Creators have no standard, safe distribution format either, so quality and safety vary
wildly.

Existing tools solve fragments. Theme installers automate copying. KDE's System Settings
applies one theme at a time. Neither provides a *transaction*: a reviewable plan, a
snapshot, verified apply, and a complete rollback.

## 2. Target users

### Primary

- **KDE Plasma 6 users who customize their desktop** but are wary of shell scripts.
  They want a different look without risking a broken session.
- **Linux users switching from macOS or Windows** who want a familiar layout and are not
  yet comfortable editing KDE configuration files.
- **Developers and technical users** who want reproducible, inspectable desktop setups
  and care about exactly what changed.

### Secondary

- **Theme and pack creators** who want a declarative, portable distribution format with
  validation instead of a bash script.
- **KDE contributors and distro maintainers** evaluating whether a customization is safe
  to recommend.
- **Open-source reviewers and security researchers** who need a documented trust model
  and an auditable transaction log.

### Not targeted in V1

- GNOME, Hyprland, XFCE, Cinnamon, or other desktop users.
- Users who want to install system packages, kernel modules, or drivers.
- Enterprise fleet management, multi-user provisioning, or policy enforcement.
- Users who expect a hosted account, cloud sync, or social features.

## 3. User pain points

| Pain point | Evidence / observation | Product response |
| --- | --- | --- |
| "I don't know what the script will do." | Installers are opaque shell | Declarative pack + resolved change plan |
| "I can't undo it." | No backups, undocumented steps | Snapshot before write; engine-owned rollback |
| "It broke my panel/shortcuts." | Direct config edits while Plasma runs | Narrow typed operations, verification, restart guidance |
| "It looked different than the screenshot." | Unversioned assets, no compatibility data | Compatibility constraints, previews, checksums |
| "I don't know where files went." | Manual copying into XDG dirs | Engine-owned install locations + history |
| "I can't trust the download." | No integrity metadata | Checksums at install, ownership tracking |
| "The theme died after a Plasma update." | Unchecked assumptions | Version constraints, compatibility gate |
| "I don't want to give a script root." | `sudo` in installers | No root, no privileged helper in V1 |

## 4. Product thesis

**If applying a desktop customization is as reviewable and reversible as a database
transaction, more people will customize their desktop, and creators will have a credible
distribution channel.**

Three claims support this:

1. **Trust is the bottleneck, not features.** Users do not need more themes; they need to
   know a theme is safe and reversible.
2. **Reversibility is an engine property, not an author property.** We cannot rely on
   pack authors to write good uninstall logic. The engine must snapshot and reverse
   everything itself.
3. **Declarative beats imperative for customization.** KDE customization is mostly
   "install these assets, set these values". A typed, declarative model is both safer and
   easier to verify than scripts.

## 5. Value proposition

For users:

- See exactly what will change, in plain language, before anything happens.
- Apply with confidence: every mutation is snapshotted and verified.
- Undo the whole change in one action, even after a crash.
- No root, no account, no telemetry, no network requirement.

For creators:

- A validated, portable pack format instead of a fragile install script.
- Automatic preview, compatibility gating, and rollback handled by the engine.
- Clear capability declarations that users can evaluate.

For the ecosystem:

- A shared, auditable definition of "safe desktop customization".
- A path to signing, a registry, and additional desktops without rewriting the engine.

## 6. Primary use cases

- **UC-1 Apply a first pack.** A Plasma 6 user opens Morphix, sees their system is
  supported, browses bundled packs, reads the change plan, and applies one.
- **UC-2 Preview without commitment.** A cautious user inspects the exact config keys and
  files a pack would touch, then declines.
- **UC-3 Roll back.** A user dislikes the result, rolls the transaction back, and gets the
  previous look, shortcuts, and effects restored.
- **UC-4 Recover from failure.** A write fails mid-apply; the engine rolls back
  automatically and explains exactly which step failed and whether restoration succeeded.
- **UC-5 Recover from a crash.** The app is killed mid-apply; on next launch it detects
  the unfinished transaction and restores the pre-apply state.
- **UC-6 Install a local pack.** A creator builds a pack directory, validates and
  installs it locally, and previews it before applying.
- **UC-7 Restore an older snapshot.** A user returns to a configuration captured before a
  previous change, possibly one made days earlier.
- **UC-8 Understand history.** A user reviews what was applied, when, and what was
  reverted, with transaction-level detail.

## 7. User journeys

### 7.1 First run

1. User launches Morphix. No account, no network prompt.
2. A status card shows distribution, Plasma version, session type, package manager, and a
   support verdict.
3. If unsupported (for example Plasma 5 or a non-KDE session), the app explains why and
   disables mutation actions; browsing is still allowed.
4. User browses the bundled catalog with previews and compatibility badges.
5. User opens a pack, sees description, assets, capabilities requested, and a
   "Preview changes" action.

### 7.2 Preview and apply

1. User clicks **Preview changes**.
2. Rust builds a plan: resolved targets, assets to install (size and destination), config
   keys with current → new values, shortcuts, KWin effects, capability list, warnings,
   and anything that requires a Plasma restart.
3. The UI displays the plan grouped by capability. Nothing has been written yet.
4. User clicks **Apply** and confirms the capability request.
5. The app acquires the mutation lock, re-validates preconditions (plan is not stale),
   snapshots every resource, writes a journal, and executes operations.
6. Progress is reported per operation. Verification runs after the last operation.
7. On success: transaction committed, history entry created, pack shown as applied.
8. On failure: reverse-order rollback runs, restoration is verified, and the UI states
   clearly whether the previous state was restored.

### 7.3 Rollback

1. User opens the applied pack or history entry and chooses **Roll back**.
2. The app shows what will be restored (assets removed, values returned, shortcuts
   restored).
3. If a later transaction modified the same resource, rollback is refused with an
   explanation and a suggested order, unless the user explicitly confirms the override.
4. Rollback executes as its own recorded transaction; result is verified and shown.

### 7.4 Local pack installation

1. User selects a pack directory.
2. The engine validates `pack.toml`: schema version, required fields, path safety, asset
   presence, checksums, capability requests, compatibility.
3. Invalid packs are rejected with field-level errors; nothing is installed.
4. Valid packs are copied into the Morphix pack store, checksums pinned, and appear in the
   catalog marked "local".
5. Applying is the same plan → apply flow as bundled packs.

### 7.5 Error and recovery

1. Any failure shows: what failed, which operation, what was completed, whether rollback
   succeeded, and where logs are.
2. If rollback itself fails, the app enters a clearly labeled **Recovery required** state,
   lists the exact affected resources with snapshot references, and offers a retry plus a
   path to the snapshot files. It never claims success it cannot verify.

## 8. Product principles

1. **Safety before customization power.**
2. **Every change is reversible.**
3. **The user sees what will change before it changes.**
4. **Packs are untrusted input.**
5. **Declarative operations, never pack-provided scripts.**
6. **Official KDE interfaces before file manipulation; documented file writes where no
   stable interface exists.**
7. **No root for normal customization.**
8. **KDE Plasma 6 only until the adapter boundary is proven.**
9. **Adapters behind explicit boundaries so future desktops are possible, not promised.**
10. **No overengineering for hypothetical futures.**
11. **Local-first.**
12. **No accounts, no cloud dependency in V1.**
13. **No untested compatibility claims.**
14. **Errors state what happened and whether rollback succeeded.**
15. **Core logic must be testable without the GUI.**

## 9. Functional requirements

Requirements are numbered for traceability to [MVP.md](MVP.md) acceptance criteria and
[TASKS.md](TASKS.md).

### System awareness

- **FR-1** Detect distribution ID, name, and version from `/etc/os-release`; tolerate
  malformed or missing fields without failing the app.
- **FR-2** Detect KDE Plasma version, session type (Wayland/X11), and whether Plasma is
  running, via DBus and read-only probes.
- **FR-3** Detect the system package manager from read-only package database files, with
  a documented fallback, and treat the result as informational only.
- **FR-4** Produce a support verdict (`Supported`, `Unsupported`, `Unverified`) with
  machine-readable reasons and human-readable explanations.

### Catalog and packs

- **FR-5** List bundled and locally installed packs with name, version, summary, preview
  image, compatibility, capabilities, and applied state.
- **FR-6** Show a pack detail view with description, attribution/license, asset list,
  compatibility, and the full declared change set.
- **FR-7** Parse `pack.toml` (schema v1) into typed models with strict unknown-field
  rejection.
- **FR-8** Validate manifests, asset paths, checksums, capability requests, and
  compatibility; reject with field-level errors.
- **FR-9** Install a valid local pack into the Morphix pack store and pin its integrity
  metadata; never execute pack content.

### Planning and preview

- **FR-10** Resolve a pack into a deterministic, ordered change plan with concrete
  targets, old → new values, installed asset sizes, required capabilities, warnings, and
  restart requirements.
- **FR-11** Detect unsupported resources, capability mismatches, target collisions, and
  incompatibilities at plan time; produce no plan when the change is unsafe.
- **FR-12** Bind every plan to a hash of its content and recorded preconditions, and
  refuse to apply a stale plan.
- **FR-13** Allow preview with zero writes to user configuration and KDE state.

### Transactions and safety

- **FR-14** Snapshot every resource that may change before the first mutation, including
  file contents, config values, and adapter-specific state; verify snapshot readability.
- **FR-15** Execute all mutations through the transaction engine under an exclusive
  mutation lock, journaling each operation's state before and after execution.
- **FR-16** Verify expected post-state after apply; commit only after verification
  succeeds.
- **FR-17** On any failure, reverse completed operations in reverse order, verify
  restoration, and report per-operation outcomes.
- **FR-18** Detect and recover interrupted transactions on startup (crash recovery).
- **FR-19** Roll back a committed transaction as a new recorded transaction.
- **FR-20** Restore a selected snapshot through the same plan → apply → verify path.
- **FR-21** Refuse rollback that would clobber resources modified by a later committed
  transaction unless the user explicitly overrides.
- **FR-22** Maintain append-only local history of plans, applies, rollbacks, restores,
  and failures.

### Permissions

- **FR-23** Define a closed set of engine capabilities; packs request a subset.
- **FR-24** Show requested capabilities with plain-language consequences and require
  explicit consent bound to the plan hash before apply.
- **FR-25** Reject any pack operation outside the declared capability set or outside the
  engine's fixed target map.
- **FR-26** Never spawn pack-provided commands, never run the app as root, and never
  escalate privileges silently.

### Persistence and operations

- **FR-27** Persist packs, installations, transactions, operations, snapshots, and
  settings in a local SQLite database with versioned migrations.
- **FR-28** Keep snapshots content-addressed and self-describing so they remain usable
  even if the database is rebuilt.
- **FR-29** Emit structured local logs with a stable error code taxonomy; include a
  diagnostics view with log location and app/system versions.
- **FR-30** Function fully offline; make no network request in V1.

## 10. Non-functional requirements

- **NFR-1 Determinism:** The same pack, system state, and manifest must produce the same
  plan (stable ordering, stable hash).
- **NFR-2 Durability:** Journal and snapshot metadata are flushed before mutations begin;
  a crash at any point leaves a recoverable state.
- **NFR-3 Performance:** Plan generation for a typical bundled pack completes in under
  2 seconds on a modern machine; apply overhead excluding asset copying is under
  1 second; UI remains responsive during long operations via progress events.
- **NFR-4 Concurrency:** Only one mutation at a time, enforced by an exclusive lock;
  concurrent previews are allowed; concurrent applies fail fast with a clear error.
- **NFR-5 Footprint:** Snapshots store only changed resources; no full-home backups.
- **NFR-6 Privacy:** No telemetry, no accounts, no network access in V1. Logs stay local
  and avoid recording file contents.
- **NFR-7 Accessibility:** Keyboard navigable UI, visible focus, semantic headings, and
  contrast-checked colors in both light and dark UI themes.
- **NFR-8 Portability of core:** `morphix-core` builds and tests on Linux without Tauri,
  a display server, or a running Plasma session (adapter integration tests excluded).
- **NFR-9 Robustness:** Hostile or malformed pack input must never panic the app; all
  parser and validator paths return typed errors.
- **NFR-10 Maintainability:** No module larger than ~600 lines of non-test code without
  an explicit justification; no catch-all `utils` module.

## 11. Supported platforms

| Dimension | V1 support |
| --- | --- |
| OS | Linux |
| Desktop | KDE Plasma 6.x (`>=6.0, <7.0`) |
| Session | Wayland (validated), X11 (accepted, not yet validated) |
| Architecture | x86_64 (primary, validated on one machine); aarch64 not validated in V0.1 |
| Distribution | Any distro shipping Plasma 6; validated distros listed per release |
| Validated combination | Plasma 6.8.0 + KF 6.31.0 + Qt 6.12.0 + Wayland, x86_64, single screen |
| Privileges | Normal user session only; root is unsupported |
| Network | Not required, not used |

Unsupported and refused in V1: Plasma 5, GNOME, Hyprland, XFCE, Cinnamon, MATE, Budgie,
COSMIC, Windows, macOS, WSL, containers without a Plasma session, and remote sessions
without a reachable session DBus.

**Supported is not the same as validated.** Any 6.x desktop may run Morphix, but only
combinations in the validated matrix ([MVP.md](MVP.md) §1.1) are reported as supported.
Other 6.x minors, X11 and multi-screen setups report `unverified` with a named warning and
remain usable. This keeps PRD principle 13 ("no untested compatibility claims") true while
the matrix is being filled in, and it is why the V1 range is stated as `>=6.0, <7.0` rather
than as a set of validated versions.

## 12. Explicit non-goals

- Not a Windhawk port; no Windhawk code, formats, or Windows concepts.
- No arbitrary shell execution, no pack scripts, no `curl | bash`.
- No system package installation, driver changes, kernel changes, or system-wide file
  writes.
- No privileged helper, daemon, or setuid component in V0.1.
- No GNOME/Hyprland/XFCE support in V1.
- No whole-layout replacement in V0.1: a theme or pack preserves the user's panel layout
  and never triggers `--resetLayout` (ADR-018, ADR-019).
- No online registry, accounts, cloud sync, telemetry, or social features in V1.
- No pack monetization.
- No support for applying a global theme as an opaque all-or-nothing unit; it is always
  decomposed into per-component operations (ADR-019).
- No automatic updates of applied packs; updates are explicit.
- No multi-user or fleet management.
- No mobile, web, or Windows/macOS builds.

## 13. Success metrics

V1 has no telemetry. Metrics are derived from (a) local transaction history the user can
view and optionally export, (b) an opt-in diagnostics export, and (c) public issue and
survey data. Targets are validation goals, not measured results.

| Metric | Definition | Target |
| --- | --- | --- |
| Installation success | App launches and reaches a support verdict on a supported system | ≥ 95% of reported attempts |
| Pack application success | Committed transactions / apply attempts on supported systems | ≥ 95% |
| Rollback success | Verified rollbacks / rollback attempts | 100% in automated tests; ≥ 99% reported |
| Second-pack usage | Users who apply a second pack within 7 days of the first | ≥ 40% of opted-in reporters |
| Failure transparency | Failed transactions where the UI reported rollback outcome | 100% (product invariant) |
| Recovery success | Interrupted transactions recovered without manual file edits | ≥ 95% |
| Time to first apply | Launch → committed first transaction | Median < 3 minutes |
| Pack author adoption | External packs validated with `pack.toml` schema v1 | ≥ 5 in the first quarter after release |
| User requests for additional packs | Distinct pack requests in the issue tracker | Tracked, not gamed |

Anti-metrics (must not increase): failed rollbacks, crash-loop launches, support issues
caused by stale plans, and packs requiring manual cleanup.

## 14. Assumptions

- **A-1** KDE Plasma 6 configuration files under `~/.config` can be read and written
  reliably enough for snapshot/rollback, with KConfig-compatible parsing for the keys we
  allowlist. **Validated on 6.8** for read/write/delete/restore; the residual risk is
  concurrent external writers losing writes (14 of 20 lost in a stress test), which the
  engine detects and retries rather than ignoring.
- **A-2** Setting the current wallpaper through a generated Plasma script, with a reliable
  read-back. **Validated on 6.8**: `wallpaper(u)` returns the image descriptor independently
  of the write path. The manual-step fallback is removed.
- **A-3** KWin effect enablement is reliable. **Corrected on 6.8**: `kwinrc` alone plus
  `reconfigure` does not load an effect, and `loadEffect` alone does not persist it. Both
  are required, and effect ids are unprefixed (`blur`, not `kwin4_effect_blur`).
- **A-4** Keyboard shortcut changes can be applied and verified through
  `org.kde.kglobalaccel` without restarting Plasma. **Validated on 6.8** with
  `setShortcut(..., flags=SetPresent)`; flags 0/1/2 are silent no-ops.
- **A-5** Users accept a per-transaction capability confirmation instead of OS-level
  sandboxing; safety is enforced by construction (typed operations, fixed targets).
- **A-6** Bundled demo packs can achieve distinct, recognizable looks using only
  engine-capabilities in V0.1 (no panel structure changes).
- **A-7** Pack authors will accept strict schema validation and a closed capability set.
- **A-8** Symlink/TOCTOU-safe writes (no-follow open semantics, ancestor re-validation,
  atomic rename) behave consistently on the filesystems Morphix supports; this needs a
  focused prototype before Phase 10 sign-off. If it does not hold everywhere, Morphix
  refuses the affected target rather than weakening the check.
- **A-9** The keys used per capability are correct across the Plasma 6 minors in the release
  matrix. **Validated on 6.8**: `plasmarc [Theme] name` is the Plasma style selector, window
  decoration lives in `kwinrc [org.kde.kdecoration2]` (**not** `kdeglobals`), and effect ids
  must be read from `/Effects.listOfEffects` because the set changes between minors
  (`contrast` no longer exists). Each release re-runs the prototype suite and downgrades any
  capability that regresses.
- **A-10** `~/.config/kdedefaults/` is user state that global-theme application rewrites.
  **Validated on 6.8**; snapshots include it. Omitting it silently loses the distro's
  default look after a rollback.
- **A-11** Panel and widget identity is not stable across removal and re-addition
  (Plasma assigns new ids). Restore is by type, index and configuration, and the plan says
  so rather than promising identity-preserving restoration.

## 15. Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| KDE config write semantics differ from assumptions (concurrent writers, `$` escaping) | Medium | High | Prototype in Phase 2; KConfig-aware parser; whole-file snapshots; limited key allowlist; integration tests on multiple Plasma minor versions |
| Wallpaper scripting API unstable across Plasma 6 minors | Medium | Medium | Isolate behind adapter capability; feature-gate with clear unsupported message; fallback to manual selection |
| Rollback conflicts with later packs | Medium | Medium | Reverse-chronological conflict detection; explicit override; no silent overwrite |
| User applies pack, then edits config manually, then rolls back | High | Medium | Precondition hashes, drift detection, explicit "changed since" warnings |
| Pack ecosystem ships unsafe content | Medium | High | Declarative-only format, checksums, capability caps, schema validation, future signing |
| Snapshot storage growth | Low | Medium | Content-addressed dedup; per-resource capture; retention policy with explicit user control |
| Plasma minor-version regressions | Medium | High | Compatibility constraints; tested-version matrix; adapter probe reports capability gaps |
| Scope creep into other desktops | High | Medium | Architecture boundary + AGENTS.md rules + ADR required to expand |
| Users expect panel/layout mods in V0.1 | Medium | Medium | Documentation, clear plan messaging, roadmap visibility |
| SQLite corruption | Low | High | WAL, migrations, snapshot manifests readable without DB, diagnostics export |

## 16. MVP acceptance criteria

The MVP is accepted only when every criterion in [MVP.md](MVP.md) §7 is satisfied, the
security failure tests in [TESTING.md](TESTING.md) pass, and no document in this
repository claims a capability that the build does not implement.

## 17. Post-MVP possibilities

Uncommitted and unordered. Each requires an ADR before implementation.

- Pack signing and a signed local trust store.
- Optional read-only pack registry with user-initiated downloads (still no accounts).
- Panel and Plasma layout capabilities via a version-aware containment model.
- Additional desktop adapters (GNOME first) once the adapter contract is proven.
- Pack updates with diffed plans and explicit upgrade transactions.
- CLI (`morphixctl`) exposing the same core to scripts and CI.
- Export/import of a full desktop profile (ordered, reproducible transactions).
- Snapshot retention policies and manual snapshot tagging.
- Localization.
- A minimal privileged helper, only if a validated need appears, designed as a separate
  broker with a narrow typed API — never the full app as root.
