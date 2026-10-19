# Morphix — Implementation Phases

- **Status:** Phase 0 complete; hosted CI run `39295512014` is green. Phase 1 (system
  detection) is starting. See [TASKS.md](TASKS.md) for the per-task state.
- **Related:** [MVP.md](MVP.md) §7 (acceptance criteria), [TASKS.md](TASKS.md)
  (executable tasks), [ARCHITECTURE.md](ARCHITECTURE.md).

No dates or estimates. A phase is complete only when every exit criterion is demonstrably
satisfied and the documentation reflects the build.

```
Phase 0  Repository foundation
   |
Phase 1  System detection
   |
Phase 2  Pack spec, parser, validator, store
   |
Phase 3  Change planner
   |
Phase 4  Snapshot engine
   |
Phase 5  Transaction and rollback engine
   |
Phase 6  KDE adapter
   |
Phase 7  Tauri integration
   |
Phase 8  React UI
   |
Phase 9  Bundled demo packs
   |
Phase 10 Security hardening
   |
Phase 11 Testing and release
```

Phases are sequential by default. Phase 9 pack *asset authoring* may start during Phase 6,
and Phase 8 UI layout may be prototyped during Phase 7, but a phase's exit criteria cannot
be met by work from a later phase.

## Phase 0 — Repository foundation

**Execution plan:** [PHASE0.md](PHASE0.md) sequences the tasks below into reviewable
stages, lists the verification commands, and records the environment prerequisites.

**Goal:** A buildable workspace with enforced boundaries, CI, a placeholder shell, and the
documentation baseline in place.

**Deliverables**

- Git repository with `.gitignore`, `.editorconfig`, `rust-toolchain.toml`, `rustfmt.toml`,
  and license files (`LICENSE-MIT`, `LICENSE-APACHE`).
- Cargo workspace: `crates/morphix-core` and `src-tauri` (package `morphix-app`).
- `morphix-core` module skeleton matching [ARCHITECTURE.md](ARCHITECTURE.md) §4, with empty
  modules and doc comments describing ownership.
- Error taxonomy: `ErrorCode` enum, `MorphixError`, serialization tests.
- Vite + React + TypeScript app with router, placeholder routes, design tokens.
- Tauri 2 shell that opens a window and answers `get_app_info`.
- `tests/architecture.rs` enforcing allowed module edges.
- CI: Rust fmt/clippy/test, frontend typecheck/test/build, docs link check.
- Docs governance: ADR template, document index, consistency-check script.

**Files / modules**

```
Cargo.toml, rust-toolchain.toml, rustfmt.toml, .editorconfig, .gitignore
LICENSE-MIT, LICENSE-APACHE
crates/morphix-core/Cargo.toml
crates/morphix-core/src/{lib.rs,errors/**,core/**,fs/**,platform/**,desktop/**,
                         packs/**,permissions/**,planning/**,snapshots/**,
                         transactions/**,storage/**,system/**,logging.rs}
crates/morphix-core/tests/architecture.rs
src-tauri/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json}
src-tauri/src/{main.rs,lib.rs,state.rs,error.rs,events.rs,commands/**}
src/{main.tsx,App.tsx,routes/**,api/**,styles/**}
.github/workflows/ci.yml
```

**Tests**

- Workspace builds; placeholder core test passes.
- `architecture` test fails on a deliberately introduced forbidden import (verified by a
  meta-test with a fixture graph, not by breaking the real tree).
- Frontend smoke test renders the shell; typecheck passes.
- Tauri `get_app_info` command test returns version and paths.
- Docs checker detects a broken link and a capability-name mismatch in a fixture.

**Exit criteria**

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `npm run typecheck`,
  `npm run test`, `npm run build` pass locally and in CI.
- The app launches on Linux and shows the placeholder UI with no filesystem or DBus access.
- Every document in the repository is linked from README, and no sidebar doc claims an
  implemented feature.
- ADR template and docs consistency check exist and run in CI.

**Dependencies:** none.

## Phase 1 — System detection

**Goal:** Accurately report whether the machine can run Morphix, with machine-readable
reasons, and render that verdict.

**Deliverables**

- `platform::paths` (XDG + env overrides), `os_release`, `package_db`, `session`.
- `platform::probe::CommandRunner` for the two allowlisted version probes (fixed argv,
  timeout, binary permission checks).
- `desktop::DesktopAdapter` trait skeleton, `AdapterProbe`, `FakeAdapter`.
- `desktop::kde::probe`: Plasma version, session type, plasmashell liveness, config file
  presence, capability matrix skeleton.
- `system::detect` and `system::support` with a `SupportLevel` and a closed reason enum.
- `SystemStatus` DTO, `get_system_status(refresh)` command, short-lived caching.
- Dashboard status card and unsupported banner (read-only catalog remains reachable).

**Files / modules**

```
crates/morphix-core/src/platform/{paths,os_release,package_db,session}.rs
crates/morphix-core/src/desktop/{mod,probe}.rs
crates/morphix-core/src/desktop/kde/probe.rs
crates/morphix-core/src/system/{detect,support}.rs
src-tauri/src/commands/system.rs
src/routes/Dashboard.tsx
src/components/SystemStatusCard.tsx
tests/fixtures/os-release/*.txt, tests/fixtures/package-db/**
```

**Tests**

- `/etc/os-release` fixtures: 5 distros plus missing/malformed/empty.
- Version parsing and range evaluation (AC-02).
- Package manager detection from file fixtures, with `Unknown` fallback (AC-03).
- Support verdict matrix: every reason code, root refusal, no session bus (AC-04).
- Probe with fake DBus: name present/absent, timeout, malformed version.
- CommandRunner: exact argv, no shell, timeout, non-root-owned binary skipped.
- Dashboard component: supported/unsupported/unverified states.

**Exit criteria**

- AC-01 through AC-04 pass.
- On a real Plasma 6 session the verdict is `Supported`; on a Plasma 5 VM it is
  `Unsupported` with the correct reason.
- Detection performs no writes (verified by `RecordingFileSystem`).
- No mutation command exists yet; UI actions that would mutate are hidden.

**Dependencies:** Phase 0.

## Phase 2 — Pack specification, parser, validator, store

**Goal:** Accept only well-formed, safe, declarative packs, and reject everything else with
precise errors.

**Deliverables**

- SQLite bootstrap: `0001_init.sql` with the full v1 schema (tables not yet used are
  created but exercised only where relevant), migration runner, `quick_check`, backup
  before migration.
- Pack models with `deny_unknown_fields`, schema-version gate, strict SemVer.
- Parser with size cap, UTF-8 enforcement, TOML error mapping.
- Path safety module for pack-relative paths (allowed prefixes, traversal, symlink/hardlink,
  executable bit, depth/length/count limits).
- Checksum parser/verifier.
- Capability enum and manifest capability cross-checks.
- Allowlist module (config targets/keys, effects, shortcuts, asset structures).
- Semantic validator with error aggregation and stable codes.
- Pack store: install local pack by copy (no-follow), pin integrity metadata, register in DB.
- Catalog assembly (bundled + local + invalid entries) and pack detail DTO.
- Commands `list_packs`, `get_pack_detail`, `install_local_pack`.
- Fixture packs: minimal valid, full valid, and one per validation error.

**Files / modules**

```
crates/morphix-core/src/core/{ids,versions,capability,resource}.rs
crates/morphix-core/src/packs/{manifest,parser,compatibility,validator,integrity,store,catalog}.rs
crates/morphix-core/src/permissions/{allowlist,resolve}.rs
crates/morphix-core/src/fs/{sandbox,tree}.rs (read-only subset)
crates/morphix-core/src/storage/{db,migrations/0001_init.sql,repo/packs}.rs
crates/morphix-core/src/storage/repo/settings.rs
src-tauri/src/commands/packs.rs
tests/fixtures/packs/**
```

**Tests**

- One fixture per error code in [PACK_SPEC.md](PACK_SPEC.md) §8.
- Path property tests: no accepted path escapes the pack root.
- Checksums: valid, mismatch, incomplete, extra, malformed, absolute/traversal entries.
- Install: valid pack copied, hashes pinned; invalid pack writes nothing (sentinel tree
  unchanged); duplicate ID/version refused.
- Catalog: bundled + local + invalid entries; applied state join.
- Fuzz harness for the TOML parser, run for 60 s in CI.

**Exit criteria**

- AC-05, AC-06, AC-07, AC-08, AC-18, AC-19 pass.
- Every validation fixture produces the documented `ErrorCode`.
- No pack is installed by execution or extraction; only directory copy.
- DB migrations apply to a fresh file and are idempotent on restart.

**Dependencies:** Phase 1.

## Phase 3 — Change planner

**Goal:** Turn a validated pack into a deterministic, read-only, fully resolved change plan
that a user can review and that can later be applied safely.

**Deliverables**

- Plan/operation models with canonical JSON and `PlanHash`.
- `SystemFingerprint` and precondition capture (pack hash, system, per-target state).
- Appearance expansion to operations using the fixed target map.
- Collision/ownership checks against `installed_assets` and filesystem state.
- Capability grant resolution and plan warnings (`manual_step`, `restart_required`,
  `overwrites_existing_value`).
- Human-readable diffs (old → new) generated in Rust.
- Plan persistence (`plans` table), expiry, discard, GC.
- Commands `create_pack_plan`, `create_snapshot_restore_plan` (stub until Phase 4),
  `discard_plan`.
- Plan rejection paths for unsupported resources and stale/incompatible packs.

**Files / modules**

```
crates/morphix-core/src/planning/{plan,planner,expand,diff,collision}.rs
crates/morphix-core/src/permissions/consent.rs (hash/token shape only)
crates/morphix-core/src/desktop/kde/targets.rs (target map)
crates/morphix-core/src/storage/repo/transactions.rs (plans queries)
src-tauri/src/commands/plans.rs
```

**Tests**

- Plan determinism and stable hashes (AC-09).
- Plan purity with recording FS/adapter (AC-10).
- Expansion for every `[appearance]` field and for settings/effects/shortcuts.
- Compatibility refusal, capability refusal, duplicate/collision refusal (AC-07, AC-21).
- Precondition capture changes ⇒ different hash; unchanged ⇒ identical.
- Expiry and GC.
- Wallpaper plans include a per-desktop `SetWallpaper` operation with an independent
  read-back channel (AC-24).
- Panel, widget, global-theme and desktop-behaviour operations expand correctly and are
  refused when the requested mechanism is unsupported (AC-27…AC-32).

**Exit criteria**

- AC-09, AC-10, AC-21, AC-24 pass; expansion tests exist for every capability.
- Planning twice on unchanged state yields identical canonical plans.
- No planner code path can write to the filesystem or DBus (verified by types and recording
  tests).
- Plans are persisted and survive an app restart until expiry.

**Dependencies:** Phase 2.

## Phase 4 — Snapshot engine

**Goal:** Capture every resource that may change, verify the capture, store it durably and
deduplicated, and plan exact restores.

**Deliverables**

- `fs::FileSystem` trait, real implementation, sandbox, atomic writes, tree hashing and
  copying, lock helper.
- Object store with dedup and fsync ordering.
- Snapshot manifest model with self-hash and verification.
- Capture for config files, config keys, asset files/directories, absent targets, and
  wallpaper descriptors.
- Snapshot reader/verifier, partial-snapshot GC.
- DB persistence of snapshot and entry index.
- Snapshot-restore plan generation with drift warnings.

**Files / modules**

```
crates/morphix-core/src/fs/{mod,sandbox,atomic,tree,lock,temp}.rs
crates/morphix-core/src/snapshots/{manifest,capture,store,restore}.rs
crates/morphix-core/src/storage/repo/snapshots.rs
crates/morphix-core/src/desktop/kde/wallpaper.rs (read descriptor helper)
crates/morphix-core/src/commands? (none; restore plan exposed through Phase 3 command)
```

**Tests**

- Capture/verify for every resource type; absent targets; directory trees.
- Dedup across snapshots; hash integrity; manifest tamper detection.
- Disk-full, permission-denied, and truncated-object failures.
- Snapshot completion marker ordering with a journal spy.
- Restore plan exactness and drift warnings.
- Temp-home isolation (no real config touched).

**Exit criteria**

- All §4.3 tests in [TESTING.md](TESTING.md) pass.
- No apply path can start without a verified, complete snapshot (proven by transaction
  tests in Phase 5, with the seam already in place here).
- Snapshot store is readable without SQLite.

**Dependencies:** Phase 3 (uses plan resource list and target map).

## Phase 5 — Transaction and rollback engine

**Goal:** Mutate state transactionally with verified rollback and crash recovery. This is
the heart of the product and receives the most tests.

**Deliverables**

- Mutation lock with OS-level exclusivity and `E_BUSY`.
- Transaction/operation state enums with guarded transitions and DB repositories.
- Durable journal writer (intent before action, completion after fsync).
- Executor for all five operation kinds against `FileSystem` + `DesktopAdapter`.
- `TransactionEngine`: apply, verify, commit, rollback on failure, failure reports.
- Committed rollback with conflict scan and explicit override.
- Rollback-of-rollback.
- Startup recovery for every interrupted state, including `recovery_required` reports.
- History events and transaction detail queries.
- Commands `apply_plan`, `rollback_transaction`, `list_transactions`,
  `get_transaction_detail`, `list_snapshots`, `get_snapshot_detail`, `get_history`.
- Crash simulator and failure-injection test matrix.

**Files / modules**

```
crates/morphix-core/src/transactions/{mod,engine,journal,executor,rollback,recovery,outcome}.rs
crates/morphix-core/src/storage/repo/{transactions,history}.rs
crates/morphix-core/src/storage/migrations/0001_init.sql (used tables)
src-tauri/src/commands/{transactions,snapshots,history}.rs
crates/morphix-core/tests/{rollback_matrix,crash_recovery,concurrency}.rs
```

**Tests**

- Full §4.5 and §4.6 suites in [TESTING.md](TESTING.md).
- Failure at every operation boundary; commit failure; verification failure.
- Rollback conflict and override; rollback failure reporting.
- Crash at every journal and filesystem boundary; recovery idempotence.
- Concurrent apply and double-click.
- History append-only behavior and detail rendering payloads.

**Exit criteria**

- AC-11, AC-12, AC-13, AC-14, AC-15, AC-16, AC-17, AC-20, AC-22, AC-23 pass using the fake
  adapter and temporary filesystems.
- No path reports success without verification.
- Recovery is proven for every durable boundary by the crash simulator.
- All tests pass with `MORPHIX_*` overridden to temp dirs, and a sentinel home tree shows
  no writes outside intended targets.

**Dependencies:** Phases 2–4.

## Phase 6 — KDE adapter

**Goal:** Implement the real Plasma 6 behavior behind the adapter for every V0.1
capability, with prototype spikes resolving the known uncertainties before broad
implementation.

**Deliverables**

- Prototype spikes (timeboxed by exit criteria, not dates):
  1. KConfig read-modify-write with concurrent writer detection, `$e`/`[$i]` handling.
  2. Wallpaper scripting call and read-back reliability across at least two Plasma 6
     minors; documented result and fallback.
  3. KWin effect reconfigure and shortcut synchronization read-back.
- `KconfigFile` parser/serializer preserving untouched content.
- KDE target map resolution with symlink refusal and installed-name derivation.
- Config value read/write with allowlist and immutable/localized refusal.
- Asset installers per kind, ownership markers, tree hashes.
- KWin effects executor, shortcut executor with kglobalaccel, wallpaper executor with
  constrained script generation.
- Adapter probe capability matrix (`Available`/`Degraded`/`Unavailable`).
- Notifications (`KGlobalSettings`, KWin reconfigure) and error mapping.
- Real-adapter integration tests on Plasma 6 VMs, gated by env.

**Files / modules**

```
crates/morphix-core/src/desktop/kde/{mod,probe,targets,config,kconfig,effects,
                                    shortcuts,wallpaper,notify}.rs
crates/morphix-core/src/transactions/executor.rs (real adapter wiring)
crates/morphix-core/src/platform/probe.rs
crates/morphix-core/tests/kde_integration.rs
docs: DECISIONS.md updates for spike outcomes (ADR-015 if fallback needed)
```

**Tests**

- §4.7 adapter suites with `FakeBus` and temp HOME.
- Script snapshot tests for wallpaper generation, including injection attempts.
- VM integration for each capability on the oldest supported and latest stable Plasma 6
  minor in the release matrix.
- Restart plasmashell and re-read state to confirm persistence.
- Degraded paths: wallpaper unavailable, missing effect, missing kglobalaccel.

**Exit criteria**

- All adapter unit tests pass; the VM suite is green on the declared matrix, with exact
  versions recorded in the release notes.
- KConfig round-trip preserves untouched lines and modes on real files.
- If wallpaper read-back fails the spike: the capability is marked `Unavailable`, plans
  emit a manual step, and ADR-015 is updated with the fallback. The MVP acceptance criterion
  is satisfied through the documented `manual_step` branch, not hidden.
- No KDE paths or DBus calls exist outside the adapter module.

**Dependencies:** Phases 3–5.

## Phase 7 — Tauri integration

**Goal:** Expose the core through a narrow, typed IPC surface with progress events, error
reports, and locked-down webview capabilities.

**Deliverables**

- `AppState`, service construction, `spawn_blocking` command wrappers.
- Every command from [ARCHITECTURE.md](ARCHITECTURE.md) §13, plus DTO mapping.
- `ts-rs` type generation and drift check.
- Progress and finished events; recovery-required event at startup.
- Startup recovery invocation before the UI can mutate.
- Tauri capabilities/CSP: no shell, fs, http plugins; local content only.
- Frontend API client (`invoke` only here), typed errors, query hooks.
- Command-level tests for happy and failure paths.

**Files / modules**

```
src-tauri/src/{lib,state,error,events}.rs
src-tauri/src/commands/{mod,system,packs,plans,transactions,snapshots,history,diagnostics}.rs
src-tauri/capabilities/default.json
src-tauri/tauri.conf.json (CSP, bundle)
src/api/{client.ts,types.ts,events.ts}
src/hooks/**
```

**Tests**

- Every command's DTO and error mapping.
- Event payload snapshots.
- Startup recovery runs before the first mutation command.
- `ts-rs` regeneration produces no diff in CI.
- A headless integration test drives plan → apply → rollback through commands with a fake
  adapter.

**Exit criteria**

- AC-20 and the IPC halves of AC-17/AC-23 pass.
- The full plan → apply → rollback flow works through commands without the UI.
- No Tauri plugin exposes filesystem, shell, or network to the webview.
- Recovery-required blocks mutation and is visible to the frontend.

**Dependencies:** Phase 6.

## Phase 8 — React UI

**Goal:** A calm, explicit UI where the change plan and its consequences are the center of
the experience, and failure states are impossible to misread.

**Spec:** [UI_SPEC.md](UI_SPEC.md) fixes every screen, state, copy string and error
presentation; it is the reference for the deliverables and tests below.

**Deliverables**

- App shell, navigation, routing, error boundary, design tokens, light/dark/system theme.
- Dashboard, Discover, Pack Detail, Preview, Outcome, Transactions, Snapshots, Settings,
  Diagnostics.
- Consent dialog bound to plan hash and capability list.
- Apply progress, success, rollback-succeeded, rollback-failed, recovery-required states.
- Rollback scope dialog with conflict list and typed override confirmation.
- Local pack install flow with field-level validation errors.
- Empty, loading, offline (always), and unsupported states.
- Accessibility pass.

**Files / modules**

```
src/routes/{Dashboard,Discover,PackDetail,Preview,Transactions,Snapshots,Settings,Diagnostics}.tsx
src/components/{PlanView,CapabilityList,ConsentDialog,ApplyProgress,Outcome,
                RollbackDialog,RecoveryBanner,ErrorBanner,PackCard,SnapshotCard}.tsx
src/hooks/{usePacks,usePlan,useApply,useSystemStatus}.ts
src/styles/**
```

**Tests**

- §4.9 suites; component tests with mocked IPC.
- Consent gating: apply disabled until acknowledged; stale hash disables apply.
- Failure states: rollback failed and recovery required cannot be dismissed as success.
- keyboard-only flow through discover → preview → consent → apply → outcome.
- XSS escaping and CSP review.

**Exit criteria**

- AC-04 through AC-17 are demonstrable through the UI.
- Every error code has a human-readable presentation; no raw stack traces.
- No component calls `invoke` outside `api/client.ts`; no KDE paths in `src/`.
- Accessibility checklist passes (keyboard, focus, labels, contrast).

**Dependencies:** Phase 7.

## Phase 9 — Bundled demo packs

**Goal:** Ship four original, licensed, useful demonstration packs that exercise every
V0.1 capability and roll back cleanly.

**Deliverables**

- `packs/cupertino`, `packs/windows-inspired`, `packs/minimal-developer`, `packs/glass`.
- Original assets for icons, cursors, color schemes, Plasma styles, Aurorae decorations,
  wallpapers, and preview screenshots.
- `README.md` and `ATTRIBUTION.md` per pack; checksums generated in CI.
- Resource bundling into the Tauri artifact and verified catalog registration.
- Pack validation and asset-license audit in CI.

**Files / modules**

```
packs/<pack>/{pack.toml,README.md,ATTRIBUTION.md,checksums.sha256,previews/,
              themes/,icons/,cursors/,plasma/,wallpapers/}
resources/packs/** (bundle input)
scripts/generate-checksums.sh
crates/morphix-core/tests/bundled_packs.rs
```

**Tests**

- All four validate with zero errors.
- Each plans, applies, and rolls back on a Plasma 6 VM (AC-26).
- Every declared capability is exercised across the set; no unreferenced files.
- No proprietary brand assets: an asset audit test checks filenames/metadata and a manual
  review records provenance; no Apple/Microsoft fonts, logos, or wallpapers.
- Preview images render and match their captions.

**Exit criteria**

- AC-26 passes for all four packs.
- A fresh Plasma 6 user applies each pack, sees the intended look (with documented manual
  panel steps where applicable), and rolls back to an unchanged state.
- Pack assets are original or compatibly licensed, with attribution.
- Pack READMEs state exactly what is automated and what is manual.

**Dependencies:** Phase 8 (planning/apply through UI); asset authoring may begin in Phase 6.

## Phase 10 — Security hardening

**Goal:** Convert the threat model into verified, automated defenses; close gaps found by
fuzzing, review, and adversarial fixtures.

**Deliverables**

- Threat-by-threat audit against [SECURITY.md](SECURITY.md) §6, with a test or accepted
  risk for each.
- Fuzzing: manifest parser, checksum parser, path validator, snapshot manifest reader.
- No-follow/TOCTOU hardening verified on real filesystems.
- Limit enforcement tests (sizes, counts, depth, config size, disk space).
- CSP and Tauri capability audit; static checks for process spawning, `sudo`, shell strings,
  `dangerouslySetInnerHTML`.
- Dependency audit (`cargo deny`, `npm audit`), license review, `unsafe` ban.
- Log redaction review.
- Performance benchmarks with budget enforcement.
- Crash/recovery soak test.

**Files / modules**

```
crates/morphix-core/fuzz/**
tests/fixtures/packs/** (security set)
tests/security_static/** (CI scripts)
.github/workflows/{ci,nightly}.yml
docs updates: SECURITY.md results, DECISIONS.md accepted risks
benches/{plan,apply,snapshot}.rs
```

**Tests**

- All §4.4 and §5 failure tests; fuzz targets run long enough to satisfy exit criteria.
- Static and configuration checks.
- Benchmark assertions: plan < 2 s, apply overhead < 1 s on the reference machine.
- Recovery soak with randomized crash points.

**Exit criteria**

- No open High/Critical threat without a documented mitigation or accepted risk.
- Fuzzing finds no panic, unbounded allocation, or path escape.
- Performance budgets met and recorded.
- Security review checklist in [SECURITY.md](SECURITY.md) §16 signed off.

**Dependencies:** Phases 5–9.

## Phase 11 — Testing and release

**Goal:** Prove the MVP on real systems, package it, and publish an honest release.

**Deliverables**

- Acceptance-criteria traceability matrix (AC-01…AC-32 → tests), CI-checked.
- VM matrix run: oldest supported and latest stable Plasma 6 minors, at least two distros,
  Wayland and X11.
- Packaging: AppImage and at least one distro package; desktop entry, icons, metadata.
- Fresh-VM end-to-end drill script and recorded results.
- Diagnostics view with local counters (no telemetry).
- Release checklist, known limitations, release notes, artifact checksums.
- Documentation sync; README status changes from "design phase" to the actual state.
- Issue templates (bug, pack request) and security reporting instructions.

**Files / modules**

```
tests/e2e/{fresh-vm.sh,apply-rollback.sh,crash-recovery.sh}
tests/AC_TRACEABILITY.md
scripts/{package.sh,release-checksums.sh}
src-tauri/tauri.conf.json (bundle targets)
.github/workflows/release.yml
docs: README.md, MVP.md, PHASES.md, CONTRIBUTING.md updates
```

**Tests**

- Full CI plus VM jobs green.
- E2E drill: install → browse → preview → apply → restart → verify → rollback → restore →
  crash recovery.
- Package installs and launches on a clean VM with no Rust/Node toolchain.
- Unsupported-environment drill (Plasma 5 VM) shows read-only behavior.

**Exit criteria**

- Every item in [MVP.md](MVP.md) §9 (Definition of Done) is satisfied.
- AC-01…AC-32 all pass in CI or the documented manual VM procedure.
- Artifacts are reproducible from the tag, with checksums published.
- No documentation claims a feature the build does not have; known limitations are
  explicit.
- Release notes list validated Plasma versions and distributions, plus unresolved risks.

**Dependencies:** Phases 0–10.

## Phase gate rules

For every phase:

1. Exit criteria are demonstrated, not asserted.
2. New behavior has tests at the appropriate layer; failure paths are included.
3. Documentation is updated in the same change set; a deviation from the design becomes an
   ADR, not a silent drift.
4. No phase may weaken a security control to make a test pass.
5. A phase does not start if a previous phase's exit criteria are unmet unless an ADR
   records the exception and its risk.
