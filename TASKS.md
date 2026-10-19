# Morphix — Implementation Tasks

- **Status:** Phase 0 in progress. P0-01, P0-03 and P0-04 are complete. P0-02 has produced
  the `morphix-core` half of the workspace and still needs `src-tauri`. All other boxes are
  unchecked.
- **Derived from:** [PHASES.md](PHASES.md). Order is a dependency order; do not start a
  task whose prerequisites are unchecked.
- **Scope:** V0.1, KDE Plasma 6 only.

## How to use this list

- One task should be reviewable as one focused change. Do not bundle unrelated tasks.
- Mark a task complete only when its code, tests, and documentation updates are merged.
- If a task reveals a design contradiction, stop and update the relevant document or add
  an ADR before continuing. Do not improvise architecture in code.
- Acceptance criteria references (`AC-xx`) point to [MVP.md](MVP.md) §7.
- Phase 0 sequencing, gates, and environment prerequisites: [PHASE0.md](PHASE0.md).
- Never weaken a test or a security control to complete a task.

---

## Phase 0 — Repository foundation

- [x] **P0-01** Add repository baseline files: `.gitignore`, `.editorconfig`,
      `rust-toolchain.toml` (pinned), `rustfmt.toml`, `LICENSE-MIT`, `LICENSE-APACHE`.
      *(the two license files were already in place — see ADR-017; the toolchain is pinned
      to `1.99.1`)*
- [ ] **P0-02** Create the Cargo workspace with `crates/morphix-core` and `src-tauri`;
      both packages build with `cargo check`. *(the workspace and `crates/morphix-core` are
      in place and build; `src-tauri` is pending the WebKitGTK 4.1 development headers —
      see [PHASE0.md](PHASE0.md) §2)*
- [x] **P0-03** Add the `morphix-core` module skeleton from ARCHITECTURE §4 (empty modules,
      ownership doc comments, no logic). *(the thirteen top-level modules exist with their
      allowed-dependency comments; nested files are created by the task that fills them)*
- [x] **P0-04** Implement `ErrorCode`, `MorphixError`, `Result`, and context entries; add
      serde round-trip tests for every code. *(73 codes; tests assert the registry matches
      ARCHITECTURE §12.1 and PACK_SPEC §8, and that operation states match DATABASE.md)*
- [ ] **P0-05** Add `tests/architecture.rs` enforcing the allowed module edges, including
      a meta-test that detects a forbidden fixture import.
- [ ] **P0-06** Scaffold the Vite + React + TypeScript UI with React Router routes,
      design tokens, and a smoke test.
- [ ] **P0-07** Scaffold the Tauri 2 shell with `get_app_info`, a placeholder window, and
      `src/api/client.ts` as the only `invoke` call site.
- [ ] **P0-08** Add CI workflows for Rust (fmt, clippy `-D warnings`, test), frontend
      (typecheck, test, build), and docs (internal link check).
- [ ] **P0-09** Add the `DECISIONS.md` ADR template, README doc index, and
      `tests/check_docs.py` (capability IDs, doc links, command names).
- [ ] **P0-10** Add `tracing` logging bootstrap with log path, rotation, 0600 mode, and a
      redaction test proving file contents are never logged.

**Phase exit:** CI green; app launches to placeholder; architecture test works; no domain
logic yet.

## Phase 1 — System detection

- [ ] **P1-01** Implement `platform::paths::XdgPaths` with `MORPHIX_DATA_DIR` /
      `MORPHIX_STATE_DIR` overrides and unit tests.
- [ ] **P1-02** Implement tolerant `/etc/os-release` parsing with fixtures for Arch, Fedora,
      Ubuntu, openSUSE, Debian, missing, empty, and malformed files. `[AC-01]`
- [ ] **P1-03** Implement package manager detection from pacman/dpkg/rpm/zypper database
      files, returning `Unknown` when absent. `[AC-03]`
- [ ] **P1-04** Implement session probing: UID, `XDG_SESSION_TYPE`, and root refusal
      (`E_RUNNING_AS_ROOT`). `[AC-22]`
- [ ] **P1-05** Implement `platform::probe::CommandRunner` with fixed argv, absolute binary
      candidates, root-owned/non-writable checks, 2 s timeout, and output cap.
- [ ] **P1-06** Define `DesktopAdapter`, `AdapterProbe`, capability support states, and
      `FakeAdapter` with call recording.
- [ ] **P1-07** Implement Plasma version parsing/normalization and SemVer range evaluation
      (`packs::compatibility`) with tests for partial and malformed versions. `[AC-02]`
- [ ] **P1-08** Implement KDE DBus presence probing for `org.kde.plasmashell`,
      `org.kde.KWin`, and `org.kde.kglobalaccel` behind a `KdeBus` trait.
- [ ] **P1-09** Implement the Plasma version fallback chain: package DB → fixed-argv probe;
      report `None` rather than guessing. `[AC-02]`
- [ ] **P1-10** Implement `system::support` rules and the closed reason enum with a complete
      unit-test matrix. `[AC-04]`
- [ ] **P1-11** Implement `SystemStatus` DTO, `get_system_status(refresh)`, and short-lived
      caching.
- [ ] **P1-12** Build the Dashboard status card and unsupported banner; keep catalog
      reachable in read-only mode. `[AC-04]`
- [ ] **P1-13** Add a detection suite asserting zero filesystem writes and no mutations.

**Phase exit:** AC-01…AC-04 pass; real Plasma 6 reports Supported; Plasma 5 VM reports the
correct refusal.

## Phase 2 — Pack spec, parser, validator, store

- [ ] **P2-01** Add `0001_init.sql` with the full v1 schema, indexes, and history triggers;
      implement the migration runner, `quick_check`, and pre-migration backup.
- [ ] **P2-02** Implement `PackId`, `AssetKey`, `InstalledName`, and version newtypes with
      validation and tests.
- [ ] **P2-03** Implement strict serde manifest models with `deny_unknown_fields`, schema
      version gate, and reserved-field rejection. `[AC-19]`
- [ ] **P2-04** Implement the manifest parser with 256 KiB cap, UTF-8 enforcement, TOML
      error mapping, and no panics. `[AC-19]`
- [ ] **P2-05** Implement pack path validation: relative-only, allowed prefixes,
      traversal, symlink/hardlink, executable bit, file type, depth/length limits.
      `[AC-19]`
- [ ] **P2-06** Implement `checksums.sha256` parsing, completeness checks, and verification.
      `[AC-19]`
- [ ] **P2-07** Implement the `Capability` enum with exact string IDs and serde tests.
- [ ] **P2-08** Implement allowlists: config files/groups/keys with value types, KWin
      effects, shortcut components/actions, asset structure requirements.
- [ ] **P2-09** Implement asset structural validators (`.colors`, `index.theme`,
      `cursors/`, non-empty directories, image extensions).
- [ ] **P2-10** Implement the semantic validator: capability declared/unused/undeclared,
      appearance cross-references, duplicate targets, compatibility-vs-`desktop`/Plasma
      constraints (`packs::compatibility`), required files, dependency syntax, error
      aggregation capped at 100.
- [ ] **P2-11** Implement local pack installation: no-follow directory copy into the pack
      store, manifest hash pinning, DB row, duplicate/version refusal. `[AC-18]`
- [ ] **P2-12** Implement catalog assembly (bundled + local + invalid) and pack detail DTO
      with declared change set. `[AC-05]` `[AC-06]` `[AC-08]`
- [ ] **P2-13** Add commands `list_packs`, `get_pack_detail`, `install_local_pack` with
      error mapping. `[AC-18]`
- [ ] **P2-14** Add `PackBuilder` and one fixture per validation error code from
      PACK_SPEC §8. `[AC-19]`
- [ ] **P2-15** Add the TOML fuzz harness with a seed corpus; run 60 s in CI.
- [ ] **P2-16** Add tests proving invalid installs leave the target tree byte-identical.

**Phase exit:** AC-05…AC-08, AC-18, AC-19 pass; every documented validation code is
reachable by a fixture.

## Phase 3 — Change planner

- [ ] **P3-01** Implement plan/operation models, canonical JSON serialization, and
      `PlanHash` with determinism tests. `[AC-09]`
- [ ] **P3-02** Implement `SystemFingerprint` and precondition capture (pack, system,
      config hash/key, target ownership, shortcut, effect, wallpaper).
- [ ] **P3-03** Finalize the KDE target map resolver and installed-name derivation with
      tests. `[AC-08]`
- [ ] **P3-04** Implement `[appearance]` expansion into concrete operations. `[AC-09]`
- [ ] **P3-05** Implement settings, KWin effect, and shortcut expansion with allowlist
      validation.
- [ ] **P3-06** Implement collision and ownership checks against `installed_assets` and
      filesystem state. `[AC-21]`
- [ ] **P3-07** Implement human-readable diff generation for config, shortcuts, effects,
      assets, and wallpaper. `[AC-09]`
- [ ] **P3-08** Implement plan warnings: `overwrites_existing_value`,
      `restart_required`, `manual_step`, `dependency_applied`.
- [ ] **P3-09** Persist plans with expiry, discard, and startup GC.
- [ ] **P3-10** Implement planner orchestration and all refusal paths (unsupported,
      incompatible, capability, collision, missing dependency). `[AC-07]` `[AC-21]`
- [ ] **P3-11** Add commands `create_pack_plan`, `discard_plan`; stub
      `create_snapshot_restore_plan` until Phase 4.
- [ ] **P3-12** Add plan-purity tests with `RecordingFileSystem` and `FakeAdapter`.
      `[AC-10]`
- [ ] **P3-13** Add per-capability planner tests, including the wallpaper probe-unavailable
      branch. `[AC-24]`

**Phase exit:** AC-09, AC-10, AC-21 and the planning half of AC-24 pass; plans are stable
and read-only to create.

## Phase 4 — Snapshot engine

- [ ] **P4-01** Implement the `FileSystem` trait and real read/metadata/list operations.
- [ ] **P4-02** Implement atomic writes: same-directory temp, fsync file, rename, fsync
      directory, mode preservation; tests on a temp filesystem.
- [ ] **P4-03** Implement `SafePath` construction and containment checks with symlink
      ancestor rejection and property tests.
- [ ] **P4-04** Implement deterministic tree hashing, tree copy, and tree removal with
      ownership checks.
- [ ] **P4-05** Implement the content-addressed object store with dedup and fsync ordering.
- [ ] **P4-06** Implement the snapshot manifest model, self-hash, and verification.
- [ ] **P4-07** Implement config file and per-key capture, including absent keys.
- [ ] **P4-08** Implement asset file/directory and wallpaper descriptor capture.
- [ ] **P4-09** Implement snapshot verification, partial-snapshot detection, and startup GC.
- [ ] **P4-10** Persist snapshots and snapshot-entry indexes; add detail queries.
- [ ] **P4-11** Implement snapshot-restore plan generation with drift warnings. `[AC-16]`
- [ ] **P4-12** Add failure tests: disk full, permission denied, truncated object, corrupted
      manifest.
- [ ] **P4-13** Implement the exclusive mutation lock helper (`flock`) with contention tests.

**Phase exit:** all snapshot tests in TESTING §4.3 pass; snapshot store is readable without
SQLite.

## Phase 5 — Transaction and rollback engine

- [ ] **P5-01** Implement transaction and operation state enums with guarded transitions.
- [ ] **P5-02** Implement transaction/operation repositories with parameterized SQL and
      state assertions.
- [ ] **P5-03** Implement the durable journal writer (intent before action, completion after
      fsync) with an ordering spy test.
- [ ] **P5-04** Implement the `InstallAsset` executor (stage, hash, rename, ownership marker,
      verify, conditional reverse).
- [ ] **P5-05** Implement the `WriteConfigValue` executor with read-modify-write, mode
      preservation, notification hook, verify, and conditional reverse.
- [ ] **P5-06** Implement shortcut, KWin effect, and wallpaper executors against
      `FakeAdapter`, including reverse verification.
- [ ] **P5-07** Implement apply orchestration: consent, expiry, lock, drift re-check,
      snapshot-complete gate, journaling, operation loop. `[AC-12]` `[AC-20]`
- [ ] **P5-08** Implement the verification stage and `E_VERIFY_FAILED` handling. `[AC-12]`
- [ ] **P5-09** Implement atomic commit (transaction, operations, installation, assets,
      history, snapshot link) with a commit-failure test.
- [ ] **P5-10** Implement rollback on failure: reverse order, verification, honest
      `rolled_back` reporting. `[AC-13]`
- [ ] **P5-11** Implement `transactions::rollback`: committed rollback with conflict scan,
      `E_ROLLBACK_CONFLICT`, and explicit override recorded in history.
      `[AC-14]` `[AC-15]`
- [ ] **P5-12** Implement rollback transactions with their own snapshot and support
      rollback-of-rollback.
- [ ] **P5-13** Implement history events and transaction detail queries. `[AC-17]`
- [ ] **P5-14** Implement startup recovery for every interrupted state, including
      `applying` reversal and idempotence. `[AC-23]`
- [ ] **P5-15** Implement `RecoveryReport`, the recovery file, and the
      `recovery_required` blocking state.
- [ ] **P5-16** Implement the crash simulator with hooks at every journal and operation
      boundary.
- [ ] **P5-17** Add the full failure matrix: fail at op N of M for all N, verify failure,
      commit failure, disk full, permission change, file removed. `[AC-13]`
- [ ] **P5-18** Add concurrency tests: two applies, double-click, stale lock, `E_BUSY`.
- [ ] **P5-19** Add commands `apply_plan`, `rollback_transaction`,
      `list_transactions`, `get_transaction_detail`, `list_snapshots`,
      `get_snapshot_detail`, `get_history`.
- [ ] **P5-20** Add the sentinel-home test proving no writes outside intended targets.
      `[AC-22]`

**Phase exit:** AC-11…AC-17, AC-20, AC-22, AC-23 pass with fakes; crash recovery proven at
every boundary.

## Phase 6 — KDE adapter

- [ ] **P6-01** Implement the KConfig-subset parser (sections, nested groups, comments,
      `$e`, `[$i]`, localized keys, CRLF) with fixtures.
- [ ] **P6-02** Implement the line-preserving serializer: untouched lines byte-identical,
      atomic write, mode preserved.
- [ ] **P6-03** Implement immutable-key refusal (`E_KEY_IMMUTABLE`) and localized-variant
      preservation.
- [ ] **P6-04** Implement config read/write with allowlist validation and symlinked-target
      refusal (`E_SYMLINK_TARGET`).
- [ ] **P6-05** Implement asset installers per kind with structural checks, ownership
      markers, and tree-hash verification.
- [ ] **P6-06** Integrate the final target map into the adapter and planner; add
      path-derivation tests.
- [ ] **P6-07** Implement KWin effects as a **paired** write: `kwinrc [Plugins] <id>Enabled`
      plus `/Effects.loadEffect` / `unloadEffect`; derive available ids at runtime from
      `listOfEffects` and intersect with the allowlist. Verify with `isEffectLoaded`.
      Measured: config+`reconfigure` alone does not load; DBus alone does not persist. `[AC-31]`
- [ ] **P6-08** Implement shortcut apply/reverse via `kglobalaccel.setShortcut` with
      `flags=SetPresent` (4), pre-checking `isGlobalShortcutAvailable`, and verifying with
      `shortcut()`. Flags 0/1/2 are silent no-ops.
- [ ] **P6-09** Implement the fixed-template Plasma script builder: typed value injection,
      JSON-escaped paths, enum-only properties, `currentConfigGroup` always set before
      `readConfig`/`writeConfig`, and golden-script snapshot tests. `[AC-24]`
- [ ] **P6-10** Implement per-desktop wallpaper apply/verify/restore using
      `PlasmaShell.wallpaper(u)` as the independent verification channel.
- [ ] **P6-11** Implement panel properties (location, height, alignment, floating, hiding)
      with API read-back, persistence wait, and restore-to-absent. Refuse opacity with
      `E_PANEL_OPACITY_UNSUPPORTED`. `[AC-27]`
- [ ] **P6-12** Implement panel create/remove with recorded ids and
      `[PlasmaViews][Panel N]` orphan cleanup. `[AC-29]`
- [ ] **P6-13** Implement widget add/configure/remove from the curated allowlist, with
      polling for asynchronous removal and layout snapshot/restore by type and order.
      `[AC-28]`
- [ ] **P6-14** Implement the look-and-feel composition engine: parse
      `contents/defaults`, expand into primitive operations, record deleted overrides,
      snapshot `kdedefaults/`, and detect the current theme by marker + override diff.
      Never pass `--resetLayout`. `[AC-30]`
- [ ] **P6-15** Implement cursor theme apply with the forced two-step re-apply when live
      state and config disagree. Refuse cursor size with `E_CURSOR_SIZE_UNSUPPORTED`.
- [ ] **P6-16** Implement desktop behaviour: virtual desktops via
      `VirtualDesktopManager`, hot corners/animations/switchers via `kwinrc` +
      `reconfigure`, window rules via `kwinrulesrc`. `[AC-32]`
- [ ] **P6-17** Implement the reload classifier (`live`/`reconfigure`/`restart_required`/
      `app_restart`) and the quiesce/persist wait; assert no operation of class `live`
      needs a restart, and that `systemctl`/`plasmashell --replace` never appear. `[AC-31]`
- [ ] **P6-18** Implement lost-write detection: read-back after every config write,
      bounded retry, `E_CONFIG_WRITE_RACE` on exhaustion.
- [ ] **P6-19** Implement residue cleanup for engine-created panels and desktops, with
      `E_RESIDUE_CLEANUP_FAILED` on failure.
- [ ] **P6-20** Implement the appearance-tool runner (colorscheme, cursortheme,
      desktoptheme) with validated enum arguments and a fixed absolute path list.
- [ ] **P6-21** Add env-gated real-session integration tests for all fifteen capabilities,
      including the desync trap, the persistence lag, and the concurrency race.
- [ ] **P6-22** Re-run the prototype suite on each Plasma 6 minor in the release matrix;
      record results in CAPABILITY_MATRIX.md, downgrading any capability that regresses.
- [ ] **P6-23** Record exact validated Plasma/Qt/distro/session combinations for this build.

**Phase exit:** adapter unit tests and real-session suite green; no KDE path or DBus call
exists outside `desktop::kde`; every capability in CAPABILITY_MATRIX maps to an automated
test; no capability is claimed in the UI that the matrix lists as unsupported.

## Phase 7 — Tauri integration

- [ ] **P7-01** Implement `AppState`, service construction, and blocking-worker wiring.
- [ ] **P7-02** Wire every command in ARCHITECTURE §13 with narrow DTOs.
- [ ] **P7-03** Generate TS types with `ts-rs` and add a CI drift check.
- [ ] **P7-04** Implement core-error → IPC-error mapping including rollback/recovery
      reports.
- [ ] **P7-05** Emit progress and finished events at operation granularity.
- [ ] **P7-06** Run startup recovery before any mutation command is available; emit
      `recovery-required`.
- [ ] **P7-07** Lock down Tauri capabilities and CSP: no shell/fs/http plugin, no remote
      content. `[AC-25]`
- [ ] **P7-08** Implement `api/client.ts` and query hooks; components use hooks only.
- [ ] **P7-09** Add command tests for every command's happy and error paths.
- [ ] **P7-10** Add a headless command-level E2E: plan → apply → rollback on a temp home.

**Phase exit:** full flow works through IPC without the UI; types drift-checked; recovery
blocks mutation.

## Phase 8 — React UI

- [ ] **P8-01** Build the app shell, navigation, routing, theme tokens, and error boundary.
- [ ] **P8-02** Build Dashboard with status, support reasons, and recent transactions.
- [ ] **P8-03** Build Discover with pack cards, source/compatibility badges, and empty
      states. `[AC-05]`
- [ ] **P8-04** Build Pack Detail with assets, capabilities, previews, license, attribution,
      and compatibility. `[AC-06]` `[AC-07]`
- [ ] **P8-05** Build Preview: capability groups, old → new diffs, warnings, totals, consent
      dialog bound to plan hash. `[AC-09]` `[AC-10]`
- [ ] **P8-06** Build apply progress and outcome views, including rollback-succeeded and
      rollback-failed states. `[AC-12]` `[AC-13]`
- [ ] **P8-07** Build Transactions list/detail and the rollback dialog with conflict list and
      override confirmation. `[AC-14]` `[AC-15]` `[AC-17]`
- [ ] **P8-08** Build Snapshots list/detail and the restore-preview flow. `[AC-16]`
- [ ] **P8-09** Build local pack install with field-level validation errors. `[AC-18]`
- [ ] **P8-10** Build the recovery-required screen and diagnostics view. `[AC-23]`
- [ ] **P8-11** Build Settings and diagnostics export.
- [ ] **P8-12** Accessibility pass: keyboard flows, focus management, ARIA, contrast.
- [ ] **P8-13** Add component tests for every flow and failure state.
- [ ] **P8-14** Add an XSS test proving pack text is escaped and CSP blocks remote loads.

**Phase exit:** AC-04…AC-17 demonstrable through the UI; no component calls `invoke`
directly; accessibility checklist passes.

## Phase 9 — Bundled demo packs

- [ ] **P9-01** Add the checksum-generation script and CI validation for bundled packs.
- [ ] **P9-02** Create Cupertino original assets: color schemes, icons, cursors, Aurorae
      decoration, Plasma style, wallpapers.
- [ ] **P9-03** Write Cupertino `pack.toml`, README, and ATTRIBUTION.
- [ ] **P9-04** Create the Windows Inspired pack (original assets; no Microsoft branding).
- [ ] **P9-05** Create the Minimal Developer pack (subdued palette, developer shortcuts).
- [ ] **P9-06** Create the Glass pack (blur/transparency, floating-panel guidance).
- [ ] **P9-07** Produce original preview screenshots for all four packs.
- [ ] **P9-08** Bundle packs as Tauri resources and verify catalog registration at runtime.
- [ ] **P9-09** Add a bundled-pack validation test: no errors, no unreferenced files, all
      capabilities exercised. `[AC-26]`
- [ ] **P9-10** Apply and roll back every pack on a Plasma 6 VM; record results. `[AC-26]`
- [ ] **P9-11** Audit asset licenses and attribution completeness; reject proprietary
      assets.

**Phase exit:** AC-26 passes; each pack has a truthful README (automated vs manual steps).

## Phase 10 — Security hardening

- [ ] **P10-01** Audit every threat in SECURITY §6; link a test or an accepted-risk ADR.
- [ ] **P10-02** Extend fuzzing to manifest, checksums, paths, and snapshot manifests; run
      nightly.
- [ ] **P10-03** Verify no-follow/TOCTOU behavior on real filesystems; add a deterministic
      swap test.
- [ ] **P10-04** Add limit-enforcement tests for every cap in PACK_SPEC §3 and config reads.
- [ ] **P10-05** Add static CI checks: no `std::process::Command` outside probes, no
      `sudo`/`pkexec`, no `sh -c`, no `dangerouslySetInnerHTML`.
- [ ] **P10-06** Audit Tauri capabilities and CSP; add a regression check.
- [ ] **P10-07** Run `cargo deny` and `npm audit`; triage findings and pin dependencies;
      assert no network client is linked into the apply path. `[AC-25]`
- [ ] **P10-08** Review logs and diagnostics for leakage; add redaction tests.
- [ ] **P10-09** Add benchmarks for plan, snapshot, and apply overhead; enforce budgets.
- [ ] **P10-10** Run a crash/recovery soak with randomized kill points.
- [ ] **P10-11** Record accepted residual risks in DECISIONS; update SECURITY with results.

**Phase exit:** no open High/Critical threat without mitigation or accepted risk; fuzzing
clean; performance budgets met.

## Phase 11 — Testing and release

- [ ] **P11-01** Create `tests/AC_TRACEABILITY.md` mapping AC-01…AC-32 to automated tests or
      manual procedures; add a CI check that every AC is covered.
- [ ] **P11-02** Run the VM matrix: oldest supported and latest stable Plasma 6 minor, two
      distros, Wayland and X11; record exact versions.
- [ ] **P11-03** Package the app (AppImage + one distro package) with desktop entry, icons,
      and metadata.
- [ ] **P11-04** Add E2E scripts for fresh install, apply/rollback all packs, snapshot
      restore, and crash recovery.
- [ ] **P11-05** Surface local validation counters in Diagnostics (no telemetry).
- [ ] **P11-06** Write release checklist, known limitations, and release notes.
- [ ] **P11-07** Sync README/MVP/PHASES with actual behavior; remove "design phase" claims
      where implemented.
- [ ] **P11-08** Add issue templates (bug, pack request) and security reporting instructions.
- [ ] **P11-09** Publish artifact checksums and confirm builds are reproducible from the
      tag.
- [ ] **P11-10** Run the MVP Definition of Done review and record evidence for each item.

**Phase exit:** MVP §9 satisfied; AC-01…AC-32 covered; documentation matches behavior;
CAPABILITY_MATRIX and PROTOTYPE_RESULTS are re-run on the release build and agree with the
shipped binary.
