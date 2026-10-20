# Morphix — Architecture

- **Status:** Design baseline; not implemented.
- **Describes:** the V1 architecture. V0.1 implements the subset defined in
  [MVP.md](MVP.md); every V0.1 feature uses the boundaries described here.
- **Audience:** implementers, reviewers, and future AI coding agents.

---

## 1. Goals and constraints

Goals:

1. Every mutation is planned, snapshotted, journaled, verified, and reversible.
2. Untrusted pack input can only reach a closed set of typed operations.
3. Core logic is testable headlessly, without Tauri, a display server, or Plasma.
4. Desktop-specific behavior is isolated behind one adapter boundary.
5. Failure is explicit: no partial success reported as success.

Hard constraints:

- Rust core, Tauri 2 shell, React + TypeScript + Vite UI, SQLite metadata, TOML packs,
  Serde. No Electron, no web backend, no accounts, no cloud, no network in V1.
- KDE Plasma 6 only. No generic "desktop plugin framework" beyond the adapter trait.
- No root, no privileged helper, no pack-supplied scripts or shell commands.

### 1.1 Component map

Every component requested by the product brief maps to exactly one owning module. There are
no duplicate implementation sites.

| Component | Owning module | Responsibility |
| --- | --- | --- |
| System detector | `system::{detect,support}`, `platform::{os_release,package_db,session,probe}` | Distro, Plasma version/session, package manager, support verdict |
| Rust core | `crates/morphix-core` | All domain logic; no GUI or Tauri dependency |
| Tauri IPC | `src-tauri/{commands,state,events,error}` | Narrow typed commands, progress events, DTO mapping |
| React UI | `src/` | Presentation, plan rendering, consent, failure states |
| KDE adapter | `desktop::{adapter,probe}`, `desktop::kde::*` | The only code that reads or writes KDE state |
| Pack parser | `packs::parser`, `packs::manifest` | Bounded TOML parsing into typed models |
| Pack validator | `packs::validator`, `packs::integrity` | Semantic rules, allowlist cross-checks, checksums |
| Compatibility engine | `packs::compatibility` | Parse `[compatibility]` and evaluate it against the detected Plasma version |
| Permission/capability engine | `permissions::{allowlist,resolve,consent}` | Closed capability set, key/effect/shortcut allowlists, consent tokens |
| Change planner | `planning::{planner,plan,expand,diff,collision}` | Deterministic read-only plan with preconditions |
| Transaction engine | `transactions::{engine,journal,executor}` | PREPARE/SNAPSHOT/APPLY/VERIFY/COMMIT orchestration |
| Snapshot engine | `snapshots::{capture,store,manifest,restore}` | Capture, verification, content-addressed storage |
| Rollback engine | `transactions::{rollback,recovery}` | Reverse-order reversal, conflict handling, crash recovery |
| History repository | `storage::repo::history`, `transactions::outcome` | Append-only events and transaction detail |
| SQLite persistence | `storage::{db,migrations,repo}` | Schema, migrations, all SQL |
| Filesystem abstraction | `fs::{sandbox,atomic,tree,lock}` | `SafePath`, atomic writes, hashing, mutation lock |
| Logging / error handling | `logging`, `errors::{code,report}` | Structured logs, redaction, stable codes, failure reports |

If a component above needs behavior that belongs in another row, the change is an
architecture change: update this table and add an ADR.

## 2. System context

```
+--------------------------------------------------------------------------+
|                              User session                                |
|                                                                          |
|  +-------------------+        typed IPC         +----------------------+  |
|  |  React + TS UI    | <----------------------> |  Tauri 2 shell       |  |
|  |  (render only)    |   commands / events      |  commands, state     |  |
|  +-------------------+                          +----------+-----------+  |
|                                                            |              |
|                                                            v              |
|                                     +----------------------------------+  |
|                                     |  morphix-core (Rust library)     |  |
|                                     |                                  |  |
|                                     |  detect -> validate -> plan      |  |
|                                     |  -> snapshot -> apply -> verify  |  |
|                                     |  -> commit | rollback            |  |
|                                     +---+----------+---------+---------+  |
|                                         |          |         |            |
|                        +----------------+          |         +---------+  |
|                        v                           v                   v  |
|              +-------------------+     +---------------------+  +-------+ |
|              | XDG config/data   |     | SQLite (metadata)   |  | KDE   | |
|              | under $HOME only  |     | ~/.local/share/...  |  | DBus  | |
|              +-------------------+     +---------------------+  +-------+ |
+--------------------------------------------------------------------------+
```

Trust boundaries (detailed in [SECURITY.md](SECURITY.md)):

- Pack files cross from "untrusted" to "validated data" only inside
  `packs::validator`; nothing downstream re-parses raw TOML.
- The frontend crosses from "untrusted presentation layer" to "typed request" only at the
  IPC boundary; commands accept IDs and small enums, never paths for mutation.
- The KDE adapter is the only module allowed to touch KDE state.

## 3. Workspace layout

A Cargo workspace separates the domain library from the Tauri shell. This is the one
structural change from a single `src-tauri/src` tree: it makes NFR-8 (headless core)
mechanically true instead of aspirational, and prevents a Tauri dependency from leaking
into planning/transaction code.

```
morphix/
├── Cargo.toml                     # workspace: members = ["crates/morphix-core", "src-tauri"]
├── rust-toolchain.toml
├── crates/
│   └── morphix-core/
│       ├── Cargo.toml
│       ├── src/                   # see §4
│       └── tests/                 # integration tests over the public API
├── src-tauri/
│   ├── Cargo.toml                 # package: morphix-app
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs                # entry point only
│       ├── lib.rs                 # builder, plugins, state wiring
│       ├── state.rs               # AppState, job registry
│       ├── events.rs              # progress event emission
│       ├── error.rs               # core error -> IPC error mapping
│       └── commands/
│           ├── mod.rs
│           ├── system.rs
│           ├── packs.rs
│           ├── plans.rs
│           ├── transactions.rs
│           ├── snapshots.rs
│           ├── history.rs
│           └── diagnostics.rs
├── src/                           # React UI
│   ├── main.tsx
│   ├── App.tsx
│   ├── routes/                    # Dashboard, Discover, PackDetail, Preview,
│   │                              # Transactions, Snapshots, Settings, Diagnostics
│   ├── api/                       # client.ts, types.ts (generated), events.ts
│   ├── components/
│   ├── hooks/
│   ├── styles/
│   └── lib/
├── packs/                         # four first-party demo packs
├── resources/packs/               # packs bundled into the Tauri bundle
├── tests/                         # cross-cutting fixtures (malicious packs, VMs)
└── docs (root *.md)
```

Rules:

- `crates/morphix-core` must not depend on `tauri`, `wry`, `webkit2gtk`, or any GUI crate.
- `src-tauri` must not contain domain logic; commands translate IPC → core calls.
- `src/` (UI) must not contain KDE paths, shell strings, or knowledge of file layouts
  beyond displaying what the core returns.

## 4. Core module tree

```
crates/morphix-core/src/
├── lib.rs                 # public facade: MorphixService + re-exports
├── errors/
│   ├── mod.rs             # MorphixError, Result
│   ├── code.rs            # ErrorCode enum (stable strings)
│   └── report.rs          # OperationReport, RollbackOutcome, RecoveryReport
├── core/
│   ├── mod.rs
│   ├── ids.rs             # PackId, TransactionId, SnapshotId, PlanId, OperationId
│   ├── versions.rs        # PlasmaVersion, SemVer wrapper, Constraint
│   ├── capability.rs      # Capability enum + human descriptions
│   ├── resource.rs        # ResourceKind, ResourceRef, ResourceState
│   └── clock.rs           # Clock trait (testable timestamps)
├── platform/
│   ├── mod.rs
│   ├── paths.rs           # XdgPaths, MorphixPaths, path overrides for tests
│   ├── os_release.rs      # tolerant /etc/os-release parsing
│   ├── package_db.rs      # read-only pacman/dpkg/rpm/zypper detection
│   └── session.rs         # UID, session type, env probing
├── fs/
│   ├── mod.rs             # FileSystem trait + RealFileSystem
│   ├── sandbox.rs         # RootedPath, SafePath, containment checks
│   ├── atomic.rs          # temp-file + fsync + rename + fsync(dir)
│   ├── tree.rs            # deterministic recursive hashing, copy, remove
│   ├── lock.rs            # exclusive mutation lock (flock)
│   └── temp.rs            # temp dir creation for tests/planning
├── desktop/
│   ├── mod.rs             # DesktopAdapter trait, AdapterProbe, TargetSpec
│   ├── probe.rs           # shared probe types, capability support
│   └── kde/
│       ├── mod.rs         # Kde module surface
│       ├── bus.rs         # KdeBus trait, session-bus connection, fake bus
│       ├── probe.rs       # KDE presence, config presence, Plasma version chain
│       ├── targets.rs     # fixed target map (kind -> XDG/config path)
│       ├── config.rs      # read/write config values via KconfigFile
│       ├── kconfig.rs     # KConfig-subset parser/serializer
│       ├── effects.rs     # KWin effect allowlist + reconfigure + /Effects DBus
│       ├── shortcuts.rs   # shortcut allowlist + kglobalaccel DBus
│       ├── wallpaper.rs   # per-desktop wallpaper template + read-back
│       ├── scripting.rs   # fixed-template Plasma script builder + result parsing
│       ├── panels.rs      # panel property read/write/create/remove + residue cleanup
│       ├── widgets.rs     # widget allowlist, add/configure/remove, layout snapshot
│       ├── lookandfeel.rs # package defaults parser, composition, marker detection
│       ├── behavior.rs    # electric borders, animations, window rules, switchers
│       ├── desktopprocs.rs# VirtualDesktopManager DBus
│       ├── kwininfo.rs    # supportInformation parser (decoration, cursor, edges)
│       ├── reload.rs      # per-capability apply class + quiesce/persist wait
│       └── notify.rs      # KGlobalSettings / KWin reconfigure notifications
├── packs/
│   ├── mod.rs
│   ├── manifest.rs        # serde models for schema v1
│   ├── parser.rs          # TOML -> manifest, size limits, strict fields
│   ├── compatibility.rs    # plasma range parsing + evaluation
│   ├── validator.rs       # semantic validation, capability cross-checks
│   ├── integrity.rs       # checksums.sha256 parsing + verification
│   ├── store.rs           # pack store install/list/remove (not V0.1 remove)
│   └── catalog.rs         # bundled + local catalog assembly
├── permissions/
│   ├── mod.rs
│   ├── allowlist.rs       # allowed config keys / effects / shortcuts per file
│   ├── resolve.rs         # manifest request -> granted capability set
│   └── consent.rs         # consent token bound to plan hash
├── planning/
│   ├── mod.rs
│   ├── plan.rs            # ChangePlan, PlannedOperation, PlanHash
│   ├── planner.rs         # orchestration, preconditions, collision checks
│   ├── expand.rs          # appearance sugar -> concrete operations
│   ├── diff.rs            # old -> new human-readable diffs
│   └── collision.rs       # target ownership / duplicate target detection
├── snapshots/
│   ├── mod.rs
│   ├── manifest.rs        # snapshot.json model + self-hash
│   ├── capture.rs         # resource capture before apply
│   ├── store.rs           # content-addressed object store
│   └── restore.rs         # plan generation for snapshot restore
├── transactions/
│   ├── mod.rs
│   ├── engine.rs          # PREPARE/SNAPSHOT/APPLY/VERIFY/COMMIT
│   ├── journal.rs         # durable state transitions
│   ├── executor.rs        # dispatch PlannedOperation -> adapter/fs
│   ├── rollback.rs        # committed rollback, conflict scan, override
│   ├── recovery.rs        # startup scan and repair
│   └── outcome.rs         # TransactionOutcome, FailureSummary
├── storage/
│   ├── mod.rs
│   ├── db.rs              # connection, WAL, busy timeout
│   ├── migrations/        # 0001_init.sql, ...
│   └── repo/
│       ├── packs.rs
│       ├── transactions.rs
│       ├── snapshots.rs
│       ├── history.rs
│       └── settings.rs
├── system/
│   ├── mod.rs
│   ├── detect.rs          # probes, SystemStatus DTO, short-lived cache
│   └── support.rs         # support verdict rules and closed reason set
└── logging.rs             # tracing setup, redaction, log path
```

Module size discipline: files above ~600 non-test lines require a justification in the PR
description. No `utils.rs`; helpers live beside the domain they serve.

## 5. Dependency direction

```
src-tauri commands
        |
        v
MorphixService (lib.rs facade)
   |-- system ---------> platform, desktop, fs
   |-- packs ----------> fs, core
   |-- planning -------> packs, permissions, desktop, fs
   |-- permissions ----> core
   |-- transactions ---> planning, snapshots, desktop, fs, storage
   |-- snapshots ------> fs, desktop, storage
   |-- desktop/kde ----> fs, platform
   |-- storage --------> core
   +-- errors (leaf)
```

Allowed edges (checked by `tests/architecture.rs`):

| Module | May depend on | Must never depend on |
| --- | --- | --- |
| `errors` | std, serde, thiserror | any domain module |
| `core` | errors, serde, semver | fs, desktop, storage, planning |
| `platform` | errors, core | planning, transactions, desktop |
| `fs` | errors | packs, desktop, storage, planning |
| `desktop` | errors, core, fs, platform | packs, planning, transactions, storage |
| `packs` | errors, core, fs | planning, transactions, desktop, permissions |
| `permissions` | errors, core | fs, desktop, transactions, packs |
| `planning` | errors, core, packs, permissions, desktop, fs | transactions, storage, Tauri |
| `snapshots` | errors, core, fs, desktop, storage | planning, transactions, packs |
| `transactions` | errors, core, planning, snapshots, desktop, fs, storage | Tauri, commands |
| `storage` | errors, core | desktop, planning, transactions |
| `system` | errors, core, platform, desktop, fs | transactions, packs |
| `logging` | errors | domain modules |
| `src-tauri` | morphix-core public API, tauri | direct fs/DBus access |

Additional rules:

- `desktop/kde` is the only place that knows about `~/.config/kdeglobals`, `kwinrc`,
  `kglobalshortcutsrc`, KDE XDG asset paths, or KDE DBus interfaces.
- `storage` contains all SQL. No SQL strings elsewhere.
- `planning` never writes; it only reads.
- `transactions` never parses TOML; it consumes `PlannedOperation`.
- Adapters never call the planner or the transaction engine; the engine calls them.
- No facade may be imported by a module it depends on. The service facade is a leaf that
  composes downward.

## 6. Capability and target model

Capabilities are a closed Rust enum. The manifest stores their string IDs. A pack cannot
name a path, only a capability; the engine resolves every target from a fixed table.

| Capability | Operation kinds it permits | Target resolution |
| --- | --- | --- |
| `appearance.color_scheme` | `InstallAsset`, `WriteConfigValue` | `~/.local/share/color-schemes/<installed-name>.colors`; `kdeglobals` `[General] ColorScheme` |
| `appearance.icons` | `InstallAsset`, `WriteConfigValue` | `~/.local/share/icons/<installed-name>/`; `kdeglobals` `[Icons] Theme` |
| `appearance.cursors` | `InstallAsset`, `WriteConfigValue`, `ApplyCursorTheme` | `~/.local/share/icons/<installed-name>/` (must contain `cursors/`); `~/.config/kcminputrc` `[Mouse] cursorTheme` |
| `appearance.plasma_style` | `InstallAsset`, `WriteConfigValue` | `~/.local/share/plasma/desktoptheme/<installed-name>/`; `~/.config/plasmarc` `[Theme] name` |
| `appearance.window_decorations` | `InstallAsset`, `WriteConfigValue`, `ReconfigureKWin` | `~/.local/share/aurorae/themes/<installed-name>/`; `~/.config/kwinrc` `[org.kde.kdecoration2] library`, `theme` |
| `appearance.wallpaper` | `InstallAsset`, `SetWallpaper` | `~/.local/share/morphix/assets/wallpapers/<pack-id>/<asset>.<ext>`; per-desktop Plasma script; read-back via `wallpaper(u)` |
| `appearance.look_and_feel` | `ApplyGlobalTheme` (expands to the rows above) | Installed `Plasma/LookAndFeel` package `contents/defaults`; `kdeglobals` `[KDE] LookAndFeelPackage`; `~/.config/kdedefaults/` |
| `appearance.fonts` | `WriteConfigValue` | `~/.config/kdeglobals` (allowlisted font keys only) |
| `panel.properties` | `SetPanelProperty` | Plasma scripting (`location`, `height`, `alignment`, `floating`, `hiding`); `~/.config/plasmashellrc` |
| `panel.widgets` | `AddWidget`, `ConfigureWidget`, `RemoveWidget` | Plasma scripting; `~/.config/plasma-org.kde.plasma.desktop-appletsrc` |
| `panel.create` | `CreatePanel`, `RemovePanel` | Plasma scripting; `~/.config/plasmashellrc` orphan cleanup |
| `settings.kde_global` | `WriteConfigValue` | `~/.config/kdeglobals` (allowlisted groups/keys only) |
| `settings.kwin_effects` | `WriteConfigValue`, `LoadEffect`, `UnloadEffect`, `ReconfigureKWin` | `~/.config/kwinrc` `[Plugins]` and `[Effect-<name>]` (allowlisted) + `org.kde.KWin /Effects` |
| `settings.shortcuts` | `WriteShortcut` | `~/.config/kglobalshortcutsrc` + `kglobalaccel` DBus |
| `settings.desktop_behavior` | `WriteConfigValue`, `ReconfigureKWin`, `ManageVirtualDesktop` | `~/.config/kwinrc` `[ElectricBorders]`, `[Compositing]`, `[WindowSwitcher]`, `kwinrulesrc`; `org.kde.KWin.VirtualDesktopManager` |

Capability strings are defined once in `core::capability::Capability` and serialized with
`snake_case` IDs matching this table. The docs, validator, planner, and UI use these exact
strings; [PACK_SPEC.md](PACK_SPEC.md) §6 and [MVP.md](MVP.md) §4 carry identical tables.
There are **fifteen** capabilities. All were exercised on a live Plasma 6.8 session;
the two exclusions (panel opacity, cursor size) are absent from the table rather than
present-but-broken, so a pack cannot request them.

Installed asset names are engine-derived: `<pack-slug>-<asset-key>` (lowercase ASCII).
Packs cannot choose the on-disk name, which prevents collisions and path tricks.

## 7. End-to-end flows

### 7.1 Browse and inspect

```
UI                    src-tauri                 morphix-core
|  list_packs()  -->  command  -->  catalog::list()
|                                        |-- packs::store::list_local()
|                                        |-- resources/packs (bundled manifest parse)
|                                        |-- storage::packs::installations()
|  <-- PackSummary[]  <--  serde
|  get_pack_detail(id) -->  catalog::detail(id)
|  <-- PackDetail (declared change set, compat, capabilities, previews)
```

Catalog load never writes. A bundled pack with an invalid manifest is surfaced as
`invalid` with its validation errors, not silently dropped.

### 7.2 Plan

```
create_pack_plan(pack_id)
  -> lock-free read phase
  -> packs::store::load(pack_id)              # validated manifest only
  -> system::detect()                          # cached per session, re-probed on demand
  -> permissions::resolve(manifest)            # requested subset of closed set
  -> adapter.probe()                           # supports wallpaper? config files present?
  -> expand::expand(manifest)                  # appearance sugar -> concrete ops
  -> planner: resolve targets, capture current values, collision checks,
              preconditions (hashes), diff generation, capability grouping
  -> storage: persist ephemeral plan (id, hash, serialized plan, created_at)
  -> return ChangePlan
```

Planning is read-only. It fails closed: any unknown key, unsupported capability,
unreadable target, or target collision produces no plan.

Preconditions recorded per plan:

- Pack content hash (manifest + checksums).
- System fingerprint: Plasma version, session type, adapter capability set.
- Per-target pre-state hash: config file content hash, target path absence/ownership,
  current shortcut value, current wallpaper descriptor.
- Plan expiry (default 10 minutes) and a content hash over the canonical plan.

### 7.3 Apply

```
apply_plan(plan_id, plan_hash, consent)
  -> validate consent.plan_hash == plan_hash == stored plan hash, unexpired
  -> acquire exclusive mutation lock (OS file lock + DB BEGIN IMMEDIATE)
  -> re-run precondition checks ("drift check"); mismatch -> E_PLAN_STALE (no writes)
  -> PREPARE:   persist transaction row + planned operations (state=pending)
  -> SNAPSHOT:  capture every resource, verify readability, fsync manifests
                 any failure -> abort, transaction=failed, no mutation performed
  -> APPLY:     for each op: journal applying -> execute -> journal applied
  -> VERIFY:    for each applied op: adapter/fs verification -> verified
  -> COMMIT:    transaction=committed, pack installation=applied, history event
  -> release lock
```

Failure at any point after the first mutation starts `ROLLBACK`: reverse applied ops in
reverse order, journaling each reversal, verify restoration, then record
`rolled_back` (or `rollback_failed` with a recovery report). Details and state machines:
[TRANSACTIONS.md](TRANSACTIONS.md).

### 7.4 Rollback and snapshot restore

- `rollback_transaction(tx_id)` is a single command: it creates an internal rollback
  transaction, checks for conflicts from later committed transactions, then reverses. It
  does not require a preview plan because the target state is the recorded pre-state, but
  the UI must display its scope before calling. This is the one deliberate exception to
  plan-first mutation; rationale in [TRANSACTIONS.md](TRANSACTIONS.md) §9.
- `create_snapshot_restore_plan(snapshot_id)` produces a normal plan that overwrites the
  snapshot's resources; the user previews and applies it like any pack.

### 7.5 Crash recovery

At startup, before serving any command that mutates:

1. Scan `transactions` for states in `snapshotting | applying | verifying | rolling_back`.
2. For each, load its journal and determine the last durable operation state.
3. If not committed, reverse applied operations in reverse order using recorded reverse
   data, verify, and mark the transaction `rolled_back` with `recovered = true`.
4. If reversal cannot complete, mark `recovery_required` and expose the report; mutations
   are blocked until the user acknowledges, and diagnostics point at the snapshot objects.

Recovery uses the same executor reversal path as normal rollback, so it is covered by the
same tests.

## 8. Runtime model

- Tauri commands are `async`; each calls `tokio::task::spawn_blocking` into the core,
  because core work is synchronous filesystem/DBus/SQLite work.
- `AppState` holds `Arc<MorphixService>` and a `Mutex<HashMap<PlanId, ...>>` only for
  in-flight job progress; durable state is always in SQLite.
- One mutation at a time:
  - OS-level exclusive `flock` on `~/.local/state/morphix/locks/mutation.lock`
    (released automatically if the process dies, which also makes stale locks detectable).
  - DB uses `BEGIN IMMEDIATE` for journal writes.
  - A second concurrent apply returns `E_BUSY` immediately; it does not queue silently.
- Long operations emit progress events at operation granularity; the UI can also poll
  transaction state, which is authoritative.
- Cancellation: V0.1 does not cancel mid-apply. A cancel request before `SNAPSHOT`
  completes aborts cleanly; after mutation begins, the apply runs to commit or rollback.
  This avoids partially-reversed states caused by user input.

## 9. KDE adapter

### 9.1 Trait

```rust
pub trait DesktopAdapter: Send + Sync {
    fn kind(&self) -> DesktopKind;                       // Kde
    fn probe(&self) -> Result<AdapterProbe>;             // version, session, capabilities
    fn resolve_target(&self, spec: &TargetSpec) -> Result<ResolvedTarget>;
    fn read_state(&self, res: &ResourceRef) -> Result<ResourceState>;
    fn validate_value(&self, target: &ResolvedTarget, value: &ConfigValue) -> Result<()>;
    fn notify(&self, change: &ChangeNotification) -> Result<()>;
}
```

`DesktopAdapter` deliberately excludes apply/verify/reverse of operation bodies: those are
implemented by operation executors (`transactions::executor`) that call adapter helpers.
This keeps the trait small and prevents an adapter from inventing its own transaction
semantics.

`AdapterProbe` reports, per capability, `Available | Degraded { reason } | Unavailable`.
After validation the only `Unavailable` entries are panel opacity and cursor size, which
are refused during planning. Icon theme activation is reported as
`Degraded { PlasmShellRestartRequired }`, which makes the planner emit a
`restart_required` warning rather than pretending the change is live.

Detection reads the Plasma version from the read-only package database when the distro's
format is text (pacman `local/*/desc`, dpkg `status`), then falls back to the two
allowlisted version probes (`plasmashell --version`, then `kwin_wayland --version`).
RPM/zypper databases are SQLite/BDB and are not parsed in `desktop` because SQL is owned
by `storage`; those distros go straight to the probes. A missing or unparseable version
is reported as unknown, never guessed.

### 9.2 Mechanisms

Every row below was executed on Plasma 6.8 and its verification channel observed working.
Evidence: [PROTOTYPE_RESULTS.md](PROTOTYPE_RESULTS.md).

| Capability | Mechanism | Verification channel | Apply class |
| --- | --- | --- | --- |
| Color scheme | Copy `.colors`; `plasma-apply-colorscheme` | `kdeglobals [General] ColorScheme` + `[Colors:*]` | live |
| Icons | Copy tree; `kdeglobals [Icons] Theme` | config read-back + post-restart shell pixels | restart_required |
| Cursors | Copy tree; `plasma-apply-cursortheme` | `KWin.supportInformation` `Cursor.themeName` **and** `kcminputrc` | live |
| Cursor size | `kcminputrc [Mouse] cursorSize` | none found — **unsupported** | — |
| Plasma style | Copy tree; `plasma-apply-desktoptheme` | `plasmarc [Theme] name` + shell pixels | live |
| Window decorations | Copy Aurorae tree; `kwinrc [org.kde.kdecoration2]`; `KWin.reconfigure` | `supportInformation` `Decoration.Plugin/Theme` | live (needs reconfigure) |
| Wallpaper | Copy image; per-desktop script; `readConfig` | `PlasmaShell.wallpaper(u)` map | live |
| Global theme | Decompose package `contents/defaults` into the rows above; marker `kdeglobals [KDE] LookAndFeelPackage` | per-component verification + `kdedefaults/package` | mixed (per component) |
| Fonts | `kdeglobals` allowlisted font keys | config read-back (no Plasma-chrome effect) | app_restart |
| Panel properties | Scripting `location/height/alignment/floating/hiding` | `panels()[i]` read-back; `plasmashellrc` after persistence | live |
| Panel create/remove | Scripting `new Panel()/remove()` | `panels().length`; orphan-section check | live |
| Widgets | Scripting `addWidget/readConfig/writeConfig/remove` | `panel.widgets()` (poll for async removal) | live |
| KDE settings | `kdeglobals` read-modify-write, allowlisted keys | config read-back with race detection | live |
| KWin effects | `kwinrc [Plugins]` **plus** `/Effects.loadEffect`/`unloadEffect` | `/Effects.isEffectLoaded` | live (needs both) |
| Shortcuts | `kglobalaccel.setShortcut(..., SetPresent)` | `kglobalaccel.shortcut()` read-back | live |
| Desktop behaviour | `kwinrc [ElectricBorders]`/`[Compositing]`; `VirtualDesktopManager` | `supportInformation` `Screen Edges`, DBus desktop list | live / reconfigure |

Verification levels:

- `StateVerified`: the persisted config/asset state is re-read and matches the plan. Always
  performed. Uses the **effective** value (user file + `kdedefaults` + `/etc/xdg`), not the
  raw user file.
- `LiveVerified`: the owning daemon confirms the state. Required wherever a channel exists:
  cursors, decorations, effects, wallpaper, panels, widgets, shortcuts, virtual desktops.
- `RestartVerified`: the state is only correct after a plasmashell restart. Only icons and
  some font keys fall here; the plan must say `restart_required` before consent.

### 9.3 Reload policy

Measured behaviour (matrix §3). The engine encodes this as a per-capability property, not
as ad-hoc calls:

| Class | Meaning | Capabilities |
| --- | --- | --- |
| `live` | Applies immediately; verify by read-back | color scheme, cursor theme, plasma style, wallpaper, panel properties, widgets, shortcuts, effects, hot corners, virtual desktops |
| `reconfigure` | Applies after `org.kde.KWin.reconfigure()` | window decorations, hot corners, animation keys |
| `restart_required` | Only correct after a plasmashell restart | icon theme, some font keys |
| `app_restart` | Newly started applications only | font keys |

The engine **never** restarts plasmashell. A restart attempt sequence observed in testing
hit systemd's start-rate limit and left the session without a shell; recovery required
`reset-failed` plus a manual start. `restart_required` entries are therefore applied to
config and reported as "takes effect on next login", with the plan stating this before
consent.

### 9.4 Persistence, quiesce and residue

Plasma persists panel and containment state asynchronously: the live value changed
immediately, while `plasmashellrc` still contained the previous values for roughly 15
seconds. The engine therefore:

1. Applies through the API and verifies through the API (authoritative).
2. Waits for the file backend to converge (bounded, with a timeout) **before** recording a
   post-state hash.
3. Records pre-state from the live API, not from the file, because a file read right after
   an apply may already describe the new state.
4. Deletes orphan sections created by its own operations: `[PlasmaViews][Panel N]` after a
   panel removal, `[Tiling][<uuid>]` after a desktop removal, and engine-created groups.
5. Compares semantically where Plasma normalises values: `alignment` is stored as Qt flags
   (`center` → `132`), colorschemes add an explicit key where the value was inherited, and
   KGlobalAccel rewrites the whole `[$InverseAction]` table.

Snapshot scope therefore includes `~/.config/kdedefaults/`, which
`plasma-apply-lookandfeel` rewrites as a side effect. Omitting it was observed to break
restoration of the plasma style.

### 9.3 KConfig subset

KDE configuration files are INI-like but not plain INI. V0.1 supports a documented subset:

- Sections `[Group]` and nested groups `[Group][Subgroup]`.
- Plain keys `key=value`.
- Escaped keys `key[$e]=...` are decoded on read and written back with `$e` preserved.
- Immutable keys (`[$i]`) are **never** modified; attempting to plan a change to one fails
  with `E_KEY_IMMUTABLE`.
- Localized key variants (`key[de]`) are not modified and are preserved.
- Untouched groups, keys, and comments are preserved verbatim by a line-preserving
  serializer; only the targeted key lines are rewritten.
- Values are read as raw strings; type coercion (int/bool) is validated before writing.
- Writes are atomic: temp file in the same directory, fsync, rename, fsync directory.
- File permissions are preserved; files are never made world-writable.

Known limitation (documented in [SECURITY.md](SECURITY.md)): KDE applications may have the
file open concurrently and may overwrite a change with their own atomic save. Morphix
detects drift on the keys it wrote at verification time and reports it; it cannot prevent
concurrent writers. This is why the key allowlist is small and why verification is not
skipped.

### 9.5 Plasma scripting boundary

`org.kde.plasmashell` `/PlasmaShell` `evaluateScript` is the only interface that can read
and write live panel, widget and wallpaper state. It is also **not a sandbox**: the script
context exposes `ConfigFile`, which can create, modify and delete arbitrary user
owner-ship config files (`new ConfigFile(path, false)` plus `writeEntry`/`deleteEntry` was
observed writing `~/.config/kdeglobals` and creating new files). The boundary is therefore
trust in the *generator*, not the interpreter:

- Every script is a **fixed template generated by Rust**. Pack input never becomes script
  text, not even as a string literal. Pack supply is limited to values validated against
  closed sets (panel property enums, allowlisted widget types, asset paths under the
  Morphix root).
- Values are injected through a typed builder that emits only JSON-escaped literals for
  paths, and only enum identifiers for properties.
- Script shape is constant per operation kind and covered by snapshot tests, so any change
  to the generated text is a reviewable diff.
- Read-back uses separate templates that call `desktops()`, `panels()`, `widgets()` and
  `readConfig`, never `ConfigFile`.
- `currentConfigGroup` must be set before `readConfig`/`writeConfig`; the wrong signature
  silently returns the key name as a value (observed). The builder always emits the group
  assignment.
- A script error returns a JS backtrace and a non-zero exit; the adapter maps it to
  `E_ADAPTER_UNAVAILABLE` with the backtrace truncated and redacted.

Threat-model consequences are recorded in [SECURITY.md](SECURITY.md) §7 and ADR-021.

### 9.5 Extending to another desktop

Adding GNOME later means:

1. A new ADR and a revision of this document.
2. A new `DesktopAdapter` implementation with its own target map, capability probe, and
   verification strategy.
3. New operation executors only if GNOME needs operation kinds KDE does not (for example
   extension enablement), with matching reverse semantics.
4. No changes to pack format, planner, transaction engine, snapshot store, or database.

If a proposed desktop change requires touching those layers, the adapter boundary is wrong
and the design must be revisited rather than patched.

## 10. Storage and persistence

- SQLite at `~/.local/share/morphix/db/morphix.sqlite3`, WAL mode, `foreign_keys=ON`,
  5-second busy timeout, one writer connection owned by `Storage` behind a mutex.
- Migrations are embedded SQL files applied in order inside a transaction, recorded in
  `schema_migrations`. Schema in [DATABASE.md](DATABASE.md).
- Snapshots live on disk, content-addressed, with their own `snapshot.json`; SQLite stores
  the index and references, never blobs.
- Plans are persisted ephemerally (they must survive a UI reload); expired plans are
  garbage-collected on startup and after successful apply.
- The database is a cache/index over durable files for some data, but the *owning* record
  of transaction state is SQLite; history and recovery depend on it.

Paths (overridable by `MORPHIX_DATA_DIR` / `MORPHIX_STATE_DIR` in tests):

| Purpose | Path |
| --- | --- |
| Database | `~/.local/share/morphix/db/morphix.sqlite3` |
| Pack store | `~/.local/share/morphix/packs/<pack-id>/` |
| Snapshot objects | `~/.local/share/morphix/snapshots/<snapshot-id>/objects/<sha256>` |
| Snapshot manifests | `~/.local/share/morphix/snapshots/<snapshot-id>/snapshot.json` |
| Wallpaper assets | `~/.local/share/morphix/assets/wallpapers/<pack-id>/` |
| Lock | `~/.local/state/morphix/locks/mutation.lock` |
| Logs | `~/.local/state/morphix/logs/morphix.log` |
| Recovery reports | `~/.local/state/morphix/recovery/<transaction-id>.json` |

## 11. Filesystem abstraction

`fs::FileSystem` is a trait with a real implementation and a test implementation:

```rust
pub trait FileSystem: Send + Sync {
    fn read(&self, p: &SafePath) -> Result<Vec<u8>>;
    fn metadata(&self, p: &SafePath) -> Result<Metadata>;
    fn write_atomic(&self, p: &SafePath, data: &[u8], mode: Option<u32>) -> Result<()>;
    fn create_dir_all(&self, p: &SafePath) -> Result<()>;
    fn remove_tree(&self, p: &SafePath) -> Result<()>;
    fn copy_tree(&self, src: &SafePath, dst: &SafePath) -> Result<TreeSummary>;
    fn list_tree(&self, p: &SafePath) -> Result<Vec<TreeEntry>>;
    fn rename(&self, from: &SafePath, to: &SafePath) -> Result<()>;
    fn exists(&self, p: &SafePath) -> Result<bool>;
    fn sync_dir(&self, p: &SafePath) -> Result<()>;
}
```

`SafePath` is constructed only by `fs::sandbox`, which:

1. Requires an absolute path.
2. Rejects `..` components and NUL bytes before any syscall.
3. Requires the path to be inside one of the allowlisted roots (user config dirs, user data
   dirs, Morphix data/state dirs).
4. Walks existing ancestors with `symlink_metadata` and rejects symlinks in the parent
   chain for write targets.
5. Rejects writing through a symlink at the final component.
6. Is re-validated after `canonicalize` when the path exists.

Tests use `FakeFileSystem` with fault injection (fail on Nth write, read error, permission
denied) and a `RecordingFileSystem` to prove planning performs no writes.

## 12. Errors, reports, logging

- `MorphixError { code: ErrorCode, message: String, detail: Option<ErrorDetail>, context:
  Vec<ContextEntry> }`, serialized to IPC with the same stable `code` strings.
- `ErrorCode` is a closed enum with `SCREAMING_SNAKE` serialization (for example
  `E_PLAN_STALE`). Codes are additive only; never repurposed.
- Apply/rollback failures return a structured report, not just an error:
  `FailureReport { transaction_id, failed_operation, completed: Vec<OperationOutcome>,
  rollback: RollbackOutcome, recovery: Option<RecoveryReport> }`.
  `RollbackOutcome` is `NotNeeded | Succeeded { verified: true } | Failed { ... }`.
- No error path may claim success. "Rollback succeeded" requires post-reversal verification.
- Logging uses `tracing` with a rolling file appender, level from settings, and a
  redaction layer that never logs file contents, full asset bytes, or values of keys marked
  sensitive (none currently). Log lines carry `transaction_id`, `plan_id`, and `pack_id`
  span fields.
- Panics are caught at the Tauri command boundary, logged, and returned as
  `E_INTERNAL`; core code is still expected to avoid panics (`unwrap` denied in CI).

### 12.1 Error code registry

Validation codes (`E_MANIFEST_*`, `E_INVALID_*`, `E_PATH_TRAVERSAL`, `E_SYMLINK_REJECTED`,
`E_CHECKSUM_*`, `E_CAPABILITY_*`, `E_CONFIG_*`, `E_EFFECT_NOT_ALLOWED`,
`E_SHORTCUT_NOT_ALLOWED`, `E_DEPENDENCY_*`, `E_PACK_ALREADY_INSTALLED`, and related) are
defined in [PACK_SPEC.md](PACK_SPEC.md) §8. Runtime and mutation codes:

| Code | Meaning |
| --- | --- |
| `E_UNSUPPORTED_PLATFORM` | Environment failed the support gate; mutations refused |
| `E_RUNNING_AS_ROOT` | Process is running as root; mutations refused |
| `E_INCOMPATIBLE_PACK` | Pack is incompatible with the detected Plasma version |
| `E_CAPABILITY_DENIED` | Plan uses a capability it did not declare or is not granted |
| `E_UNSUPPORTED_RESOURCE` | Resource/operation kind is outside the V0.1 target map |
| `E_TARGET_EXISTS` | Target path exists and is not owned by this pack |
| `E_SYMLINK_TARGET` | Write target or an ancestor is a symlink |
| `E_KEY_IMMUTABLE` | Config key is marked immutable by KDE |
| `E_PLAN_STALE` | Precondition changed since planning; nothing written |
| `E_PLAN_EXPIRED` | Plan TTL elapsed; re-preview required |
| `E_CONSENT_REQUIRED` | Consent missing or not bound to this plan hash |
| `E_BUSY` | Another mutation holds the lock |
| `E_INSUFFICIENT_SPACE` | Free space below snapshot + install + safety margin |
| `E_SNAPSHOT_UNREADABLE` | A snapshot object or manifest could not be read/verified |
| `E_SNAPSHOT_CORRUPT` | Snapshot manifest self-hash or object hash mismatch |
| `E_APPLY_FAILED` | An operation failed; rollback outcome is reported separately |
| `E_VERIFY_FAILED` | Post-state did not match the plan |
| `E_ROLLBACK_CONFLICT` | A newer change would be overwritten by rollback |
| `E_ROLLBACK_FAILED` | Reversal could not be verified; recovery report attached |
| `E_TRANSACTION_NOT_ROLLBACKABLE` | Original transaction is not committed |
| `E_PACK_ALREADY_APPLIED` | Pack is already applied; roll back before reapplying |
| `E_RECOVERY_REQUIRED` | Interrupted transaction could not be recovered; mutation blocked |
| `E_ADAPTER_UNAVAILABLE` | Required KDE interface/DBus call failed or timed out |
| `E_SCRIPT_FAILED` | A generated Plasma script returned a JS error/backtrace |
| `E_CONFIG_WRITE_RACE` | A written key did not survive (external writer); retries exhausted |
| `E_PANEL_OPACITY_UNSUPPORTED` | Pack requested panel opacity; no working mechanism exists |
| `E_CURSOR_SIZE_UNSUPPORTED` | Pack requested cursor size; no working mechanism exists |
| `E_RESTART_REQUIRED` | Change cannot be verified without a shell restart; not applied live |
| `E_PERSIST_TIMEOUT` | Plasma did not persist state within the quiesce timeout |
| `E_RESIDUE_CLEANUP_FAILED` | Engine-created panel/desktop config could not be removed |
| `E_CONFIG_TOO_LARGE` | Config file exceeds the 16 MiB read cap |
| `E_IO_PERMISSION` | Permission denied during a targeted read/write |
| `E_IO` | Other targeted I/O failure with context |
| `E_DB_CORRUPT` | SQLite `quick_check` failed; mutations blocked |
| `E_DB_NEWER_THAN_APP` | Database schema is newer than this build |
| `E_INTERNAL` | Caught panic or impossible state; never a success |

Codes are additive only. A code must not change meaning after release.

## 13. IPC surface (Tauri commands)

All commands are narrow and return typed results. No command accepts a filesystem path for
mutation except `install_local_pack` (a user-chosen directory) and
`export_diagnostics` (a user-chosen file).

| Command | Input | Output |
| --- | --- | --- |
| `get_app_info` | — | version, build, log path, data dir |
| `get_system_status` | `refresh: bool` | `SystemStatus` |
| `list_packs` | — | `PackSummary[]` |
| `get_pack_detail` | `pack_id` | `PackDetail` |
| `install_local_pack` | directory path (from OS dialog) | `PackSummary` |
| `create_pack_plan` | `pack_id` | `ChangePlan` |
| `create_snapshot_restore_plan` | `snapshot_id` | `ChangePlan` |
| `discard_plan` | `plan_id` | `()` |
| `apply_plan` | `plan_id`, `plan_hash`, `consent` | `TransactionOutcome` |
| `rollback_transaction` | `transaction_id`, `confirm_scope: bool`, `override_conflicts: bool` | `TransactionOutcome` |
| `list_transactions` | `filter`, `limit`, `offset` | `TransactionSummary[]` |
| `get_transaction_detail` | `transaction_id` | `TransactionDetail` |
| `list_snapshots` | — | `SnapshotSummary[]` |
| `get_snapshot_detail` | `snapshot_id` | `SnapshotDetail` |
| `get_history` | `filter`, `limit`, `offset` | `HistoryEntry[]` |
| `get_diagnostics` | — | local counters, versions, adapter probe, DB health |
| `export_diagnostics` | destination path (from OS dialog) | written path |

Events:

- `morphix://transaction-progress`: `{ transaction_id, stage, operation_index,
  operation_total, message }`.
- `morphix://transaction-finished`: `{ transaction_id, outcome }`.
- `morphix://recovery-required`: `{ transaction_id, report }`.

TypeScript types are generated from the Rust IPC types with `ts-rs` and committed; a CI
test regenerates and fails on drift. IPC types are separate DTOs, not internal domain
types, so internal refactors do not silently change the UI contract.

## 14. Frontend architecture

- React + TypeScript + Vite. React Router for routes. TanStack Query for IPC data fetching,
  caching, and invalidation. No global mutable store for domain state.
- Routes: Dashboard (system status, recent transactions), Discover, Pack Detail, Preview,
  Transactions, Snapshots, Settings, Diagnostics.
- `api/client.ts` is the only module that calls `invoke()`. Components never call
  `invoke()` directly, never construct KDE paths, and never write files.
- Mutation flow is explicit: create plan → render plan → collect consent → apply → render
  outcome. The Apply button is disabled until the displayed plan hash matches the plan
  being applied.
- Apply progress is rendered from events, with a polling fallback on transaction detail.
- Error rendering is centralized: `ErrorBanner` shows code, message, rollback status,
  affected resources, and the diagnostics path. A "rollback failed" outcome has a distinct,
  non-dismissable `RecoveryRequired` view.
- Styling: plain CSS with design tokens and `prefers-color-scheme`; no UI framework in
  V0.1. Accessibility: keyboard nav, focus rings, semantic landmarks.

## 15. Rust vs TypeScript

| Concern | Rust | TypeScript |
| --- | --- | --- |
| OS/distro/Plasma detection | Yes | Display only |
| Pack parsing/validation | Yes | Never |
| Capability enforcement | Yes | Display only |
| Path safety | Yes | Never |
| Planning/diffing | Yes | Render only |
| Snapshot/transaction/rollback | Yes | Never |
| SQLite/storage | Yes | Never |
| KDE DBus/config writes | Yes | Never |
| Logging of mutations | Yes | UI errors only |
| Navigation, layout, copy | — | Yes |
| Consent UI and plan presentation | Data only | Yes |
| Progress rendering | Emits events | Yes |

Rule of thumb: if it can change user state or interpret untrusted input, it is Rust.

## 16. Test seams

- `Clock` trait for deterministic timestamps.
- `FileSystem` trait with fake + fault injection.
- `DesktopAdapter` trait with `FakeAdapter` (configurable probe, recording calls) and the
  real `KdeAdapter`.
- `CommandRunner` trait used only for allowlisted version probes; fake in tests asserts
  exact argv and never a shell.
- DBus access behind a `KdeBus` trait so transaction tests do not need a session bus.
- `tests/architecture.rs` enforces module edges from §5.
- Planner and transaction engine take `&dyn` dependencies, constructed by the service
  facade; no global singletons.

## 17. Packaging and runtime dependencies

- Tauri 2 packaging: AppImage and distro packages produced from the same build; the binary
  is `morphix`.
- Runtime host requirements: Plasma 6 session, session DBus, `kglobalaccel` available.
- No bundled KDE libraries beyond what Tauri/WebKitGTK require; Morphix reads and writes
  configuration rather than linking KDE Frameworks, which keeps packaging portable.
- Bundled packs are Tauri resources, copied into the pack store read-only at first launch
  or read directly from the resource dir with a verified checksum. V0.1 reads them from the
  resource directory and registers them in the catalog; it does not copy them into user
  space until applied.
