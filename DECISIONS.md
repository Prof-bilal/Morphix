# Morphix — Architecture Decision Records

- **Status:** Living index. All records below are accepted for the V0.1 design baseline.
- **Date of baseline:** 2026-10-18.
- **Process:** see §"Adding an ADR" at the end.
- **Related:** [ARCHITECTURE.md](ARCHITECTURE.md), [PRD.md](PRD.md), [MVP.md](MVP.md).

An ADR records *why* a decision was made, what it costs, and what it forces later. If code
and an ADR disagree, the ADR is wrong until it is updated or superseded.

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [ADR-001](#adr-001--rust-for-the-core-engine) | Rust for the core engine | Accepted |
| [ADR-002](#adr-002--tauri-2-instead-of-electron) | Tauri 2 instead of Electron | Accepted |
| [ADR-003](#adr-003--react--typescript--vite-ui) | React + TypeScript + Vite UI | Accepted |
| [ADR-004](#adr-004--kde-plasma-6-first-and-only-in-v1) | KDE Plasma 6 first, and only, in V1 | Accepted |
| [ADR-005](#adr-005--declarative-packs) | Declarative packs | Accepted |
| [ADR-006](#adr-006--no-arbitrary-script-execution) | No arbitrary script execution | Accepted |
| [ADR-007](#adr-007--transactional-configuration-changes) | Transactional configuration changes | Accepted |
| [ADR-008](#adr-008--local-first-mvp) | Local-first MVP | Accepted |
| [ADR-009](#adr-009--sqlite-for-local-state) | SQLite for local state | Accepted |
| [ADR-010](#adr-010--no-pack-declared-filesystem-paths) | No pack-declared filesystem paths | Accepted |
| [ADR-011](#adr-011--defer-panel-and-plasma-layout-changes-to-post-mvp) | Defer panel and Plasma layout changes | Accepted |
| [ADR-012](#adr-012--compose-appearance-instead-of-applying-look-and-feel-packages) | Compose appearance instead of Look-and-Feel packages | Accepted |
| [ADR-013](#adr-013--separate-morphix-core-crate-from-the-tauri-shell) | Separate `morphix-core` crate from the Tauri shell | Accepted |
| [ADR-014](#adr-014--strict-schema-versioning) | Strict schema versioning | Accepted |
| [ADR-015](#adr-015--wallpaper-via-a-constrained-plasma-scripting-call) | Wallpaper via a constrained Plasma scripting call | Accepted (prototype-gated) |
| [ADR-016](#adr-016--no-root-no-daemon-no-privileged-helper-in-v1) | No root, no daemon, no privileged helper in V1 | Accepted |
| [ADR-017](#adr-017--licensing-code-and-pack-assets) | Licensing: code and pack assets | Accepted (pre-release) |
| [ADR-018](#adr-018--panel-widget-and-desktop-behaviour-customization-in-v01) | Panel, widget and desktop-behaviour customization in V0.1 | Accepted (supersedes ADR-011) |
| [ADR-019](#adr-019--global-themes-as-decomposed-plans) | Global themes as decomposed plans | Accepted (supersedes ADR-012) |
| [ADR-020](#adr-020--never-restart-the-desktop-shell) | Never restart the desktop shell | Accepted |
| [ADR-021](#adr-021--plasma-scripting-is-a-trusted-generator-boundary) | Plasma scripting is a trusted-generator boundary | Accepted |
| [ADR-022](#adr-022--verify-live-state-and-effective-configuration) | Verify live state and effective configuration | Accepted |
| [ADR-023](#adr-023--allowlist-tables-live-in-the-core-module) | Allowlist tables live in the `core` module | Accepted |
| [ADR-024](#adr-024--acceptance-criteria-follow-the-phase-that-supplies-their-dependency) | Acceptance criteria follow the phase that supplies their dependency | Accepted |

---

## ADR-001 — Rust for the core engine

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The core must read and write configuration files atomically, enforce path safety, hash
content, manage snapshots, and survive crashes without corrupting user state. It processes
untrusted input (pack manifests and assets). It is a desktop application, not a web
service, and must ship as a single binary with predictable behavior and no runtime.

### Decision

Implement the entire domain engine in Rust: detection, pack parsing/validation, planning,
snapshots, transactions, rollback, SQLite access, and the KDE adapter. The frontend is a
presentation layer only.

### Reasoning

- Memory safety without a garbage collector matters for a process that performs many
  fallible filesystem operations.
- Strong typing and exhaustive enums fit a closed operation/capability model; the compiler
  helps prevent unhandled operation kinds.
- `serde` + `toml` gives strict, typed manifest parsing; `rusqlite` and `sha2` are mature.
- Single static binary simplifies packaging and avoids a runtime dependency.
- Rust's error handling (`Result`, `thiserror`) supports the required explicit failure
  reporting.

### Tradeoffs

- Slower iteration than a scripting language for UI-adjacent logic.
- Smaller ecosystem for some KDE-specific helpers; more manual file/DBus work.
- Compile times and a steeper contributor curve.
- Async Tauri + synchronous core requires careful `spawn_blocking` boundaries.

### Consequences

- All system-sensitive code lives in `crates/morphix-core`.
- No shelling out to perform configuration writes; the engine implements them.
- Contributors need a Rust toolchain; CI must run fmt, clippy, and tests.
- The frontend cannot implement a "quick fix" that bypasses the engine.

---

## ADR-002 — Tauri 2 instead of Electron

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The product needs a native desktop window, a React UI, and a Rust core. Electron would
bundle a full Chromium and Node runtime; Tauri uses the system webview and a Rust backend
with a typed command bridge.

### Decision

Use Tauri 2 as the desktop shell. Do not use Electron.

### Reasoning

- Natural fit for a Rust core: commands and events instead of a sidecar process or a local
  HTTP server.
- Much smaller bundle and memory footprint; uses the distro's WebKitGTK.
- Capability-based permission model for exposed commands, which matches the product's
  least-privilege stance.
- No Node runtime in production.

### Tradeoffs

- Webview differences across distributions (WebKitGTK versions) can affect rendering.
- Fewer ready-made integrations than Electron; some plugins are immature.
- Debugging webview/backend interaction is less familiar to some contributors.
- Tauri's capability configuration must be understood, or it becomes an attack surface.

### Consequences

- IPC is a first-class, typed contract ([ARCHITECTURE.md](ARCHITECTURE.md) §13).
- The app must be tested on the distributions it claims to support, with their WebKitGTK.
- No Node APIs at runtime; any temptation to add a local server is rejected.
- Tauri CSP and capability configuration are security review items (SECURITY §16).

---

## ADR-003 — React + TypeScript + Vite UI

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The UI must present complex, consequential information: change plans, old → new values,
capability requests, rollback outcomes, and recovery states. Type errors in this surface
are product-safety defects, not cosmetic issues.

### Decision

Use React with TypeScript, built by Vite. No UI component framework in V0.1; plain CSS with
design tokens. TanStack Query for IPC data, React Router for routing.

### Reasoning

- TypeScript catches shape mismatches between Rust DTOs and UI rendering.
- React's component model suits plan diff views and multi-step flows.
- Vite provides fast feedback and a simple production build.
- Avoiding a heavy UI kit keeps the bundle small and the visual language intentional.
- Generated types (`ts-rs`) keep the contract honest.

### Tradeoffs

- More frontend code to write than a component kit would require.
- React state management can drift into ad-hoc global state if not constrained.
- Webview performance on low-end machines needs attention for large plans.

### Consequences

- `api/client.ts` is the only `invoke` call site; components receive typed data.
- A CI drift check keeps generated TS types in sync with Rust.
- Accessibility is a requirement, not a later polish item.
- No frontend code may contain KDE paths or mutation logic.

---

## ADR-004 — KDE Plasma 6 first, and only, in V1

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Desktop environments differ radically in configuration models. Supporting several at once
would force generic abstractions before the safety model is proven, and would spread
testing thin. KDE Plasma 6 has a large customization community, file-based configuration,
and usable DBus interfaces.

### Decision

V1 supports KDE Plasma 6 (`>=6.0, <7.0`) only. GNOME, Hyprland, XFCE, Cinnamon, and other
desktops are unsupported and must be refused, not "best effort".

### Reasoning

- A narrow target lets the transaction model, target map, and verification be exact.
- Plasma 6 is the current generation and has a substantial user base.
- Plasma 5 is end-of-life for the relevant timeframe; pretending to support both doubles
  the configuration matrix.
- A proven adapter contract is a better foundation for expansion than speculative
  generality.

### Tradeoffs

- Smaller initial addressable audience.
- Plasma minor-version differences still require a tested matrix.
- Users on other desktops get a clear refusal, which may feel exclusionary.

### Consequences

- The support verdict refuses non-Plasma environments before planning.
- The adapter boundary exists, but no second adapter is written in V1.
- Adding a desktop requires an ADR and an adapter implementation, not a config flag.
- Documentation and AGENTS.md forbid silent scope expansion.

---

## ADR-005 — Declarative packs

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Theme installers typically ship imperative shell scripts. Scripts can do anything, cannot
be meaningfully previewed, and are effectively impossible for the engine to reverse.

### Decision

Packs are declarative TOML plus assets. They select from a closed set of capabilities and
operation kinds. There is no hook, script, or executable content.

### Reasoning

- A declarative change set can be validated, resolved, diffed, previewed, and reversed by
  the engine without trusting the author.
- Declarative packs are portable across compatible Morphix versions and easier to review.
- The engine owns install locations and file naming, which prevents path attacks and
  ownership ambiguity.
- It creates a stable creator format that can later be signed and indexed.

### Tradeoffs

- Authors cannot express arbitrary tweaks; some customizations are impossible in V1.
- The schema must evolve carefully (ADR-014) as capabilities grow.
- Complex visual changes (panel layouts, KWin scripts) are out of reach initially.

### Consequences

- `PACK_SPEC.md` is a contract; schema changes are versioned.
- The validator is the only gate between untrusted files and the engine.
- "Just run this script" is never an accepted solution in issues or reviews.
- Future capabilities require new typed operations with reverse semantics.

---

## ADR-006 — No arbitrary script execution

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The easiest way to implement customization is to execute an author's script. It is also
the largest security and reversibility liability: arbitrary code can exfiltrate data,
install persistence, or break the session, and it cannot be rolled back generically.

### Decision

Morphix never executes pack-provided code. No shell, no interpreter, no binaries, no
`curl | bash`. The only child processes permitted in V0.1 are two fixed-argument version
probes used for detection. Pack content cannot influence process arguments, environment, or
working directory.

### Reasoning

- Removes the highest-severity threat class entirely rather than mitigating it.
- Makes previews honest: the plan is the complete set of changes.
- Enables generic snapshot/rollback: operations are known and reversible.
- Aligns with the product thesis that trust is the bottleneck.

### Tradeoffs

- Some customizations cannot be shipped.
- Authors may push back; the product must explain the boundary clearly.
- Internal adapter scripting (ADR-015) still exists and must be kept isolated.

### Consequences

- Executable files in packs are rejected; process APIs are restricted by CI checks.
- The wallpaper adapter's internal script template is not pack input and is reviewed as
  engine code.
- Any future need for scripts requires an ADR with a sandbox, review model, and reversal
  strategy - not a quiet exception.

---

## ADR-007 — Transactional configuration changes

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Desktop configuration is spread across several files and DBus daemons. A failure halfway
through leaves a mixed state that is hard to diagnose. Users cannot be expected to restore
files manually.

### Decision

Every mutation follows plan → snapshot → apply → verify → commit, with reverse-order
rollback on failure, durable journaling before each operation, and startup crash recovery.
Success is never reported without verification.

### Reasoning

- The product's core promise is reversibility; it must be an engine property.
- A journal makes crash outcomes recoverable instead of ambiguous.
- Verification catches silent failures (daemon rejects a shortcut, file was concurrently
  rewritten).
- Conditional reversal preserves user changes made after apply.

### Tradeoffs

- More code and more states than direct writes.
- Snapshot storage and verification add latency.
- Concurrent KDE writers can still invalidate a change after verification; the engine
  reports conflicts rather than pretending to control them.
- Rollback may be refused when newer changes conflict, which is safer but less convenient.

### Consequences

- No module outside `transactions` may write user configuration.
- Every operation kind implements validate/apply/verify/reverse.
- Tests must cover failure at every operation and crash boundary.
- [TRANSACTIONS.md](TRANSACTIONS.md) is normative for mutation behavior.

---

## ADR-008 — Local-first MVP

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

A registry or account system would add backend infrastructure, identity, moderation, and
privacy obligations before the core safety model is proven. The first release needs to
validate whether users trust and use reversible customization at all.

### Decision

V0.1 has no network access, no accounts, no cloud, no telemetry. The catalog is bundled;
local packs are installed from a directory. Diagnostics are local and exported only by an
explicit user action.

### Reasoning

- Eliminates a large attack surface and all data-protection obligations.
- Keeps the product usable offline and in restricted environments.
- Forces the pack format and transaction engine to stand on their own.
- Makes the trust story simple: nothing leaves the machine.

### Tradeoffs

- No pack discovery beyond four bundled demos; growth depends on manual sharing.
- No usage analytics; success metrics rely on opt-in reports and local diagnostics.
- No automatic updates or remote revocation of a bad pack.
- A future registry will need an ADR and a privacy/security redesign.

### Consequences

- No HTTP client in the app or core.
- Pack requests happen through the issue tracker, not an in-app marketplace.
- Signing and registry work is explicitly post-MVP.
- Tests assert offline operation and absence of network clients in the apply path.

---

## ADR-009 — SQLite for local state

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Rollback and crash recovery need durable, queryable state: transactions, operation
journals, snapshots, ownership, and history. Flat JSON files would need hand-rolled
atomicity, indexing, and migration logic.

### Decision

Use SQLite via `rusqlite` with a versioned schema, WAL mode, `synchronous=FULL`, and
embedded migrations. Snapshots and assets remain on disk; SQLite stores metadata only.

### Reasoning

- ACID transactions match the journal requirement exactly.
- Mature tooling, single file, no server, no separate daemon.
- Indexes support history, conflict scans, and recovery queries.
- Migrations can be tested and versioned like code.
- `synchronous=FULL` prioritizes journal durability over write throughput, which is the
  right tradeoff for rare mutations.

### Tradeoffs

- Another format and migration path to maintain.
- DB corruption would lose history/ownership indexing (snapshot manifests remain readable).
- SQLite is not a blob store; snapshots must not be stuffed into it.
- Single-writer serialization is acceptable but constrains future concurrency.

### Consequences

- [DATABASE.md](DATABASE.md) is the schema contract; all SQL lives in `storage`.
- Snapshot manifests are self-describing and readable without the DB.
- Backups are taken before migrations; downgrades are refused.
- History is append-only, enforced by triggers.

---

## ADR-010 — No pack-declared filesystem paths

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The original product sketch showed a pack declaring
`[permissions] filesystem = ["~/.config/kdeglobals", ...]`. That design treats paths as
capabilities. It invites traversal, symlink, ownership, and collision problems, and it makes
the engine's target map dependent on untrusted input.

### Decision

Packs never declare filesystem paths. They declare asset keys and typed changes. The engine
resolves every target from a fixed, code-owned table. Capabilities describe classes of
change, not locations.

### Reasoning

- Path validation is much easier when the set of write targets is finite.
- Users can review "this pack changes your icon theme" instead of auditing paths.
- Ownership, collision detection, and rollback are deterministic.
- It prevents a pack from touching unrelated files even if validation has a bug.

### Tradeoffs

- Less flexible than arbitrary paths; some customizations are impossible.
- New target kinds require engine changes and a release, not just a manifest change.
- Packs cannot write to unusual custom locations.

### Consequences

- The validator rejects absolute paths and any target outside the fixed map.
- Installed names are engine-derived, preventing collisions with user themes.
- Adding a target is a code review and ADR-worthy change.
- `PACK_SPEC.md` and `ARCHITECTURE.md` §6 must stay in sync; the docs checker enforces it.

---

## ADR-011 — Defer panel and Plasma layout changes to post-MVP

- **Status:** **Superseded by [ADR-018](#adr-018--panel-widget-and-desktop-behaviour-customization-in-v01)**
- **Date:** 2026-10-18
- **Superseded on:** 2026-10-18 after live capability testing

> **Supersession note.** This decision was made without testing. It was reversed after the
> Plasma scripting API (`panels()`, `new Panel()`, `panel.addWidget()`, `widget.remove()`)
> was exercised on Plasma 6.8 and proved to support property changes, panel creation and
> removal, and widget add/configure/remove with read-back and rollback. The premise that
> layout changes require either forbidden layout scripts or fragile containment editing was
> false. The original reasoning is preserved below as a record of the mistake.

### Context

Panel and full Plasma layouts are central to "looks like macOS/Windows" packs. On Plasma 6,
layout changes involve either executing Plasma layout JavaScript (forbidden by ADR-006) or
synthesizing containments/applets in `plasma-org.kde.plasma.desktop-appletsrc` with
version-fragile IDs, followed by a plasmashell restart. Neither is safely reversible with
the V0.1 operation model.

### Decision

V0.1 does not modify panel configuration or Plasma layouts. The capability set excludes
them and schema v1 has no representation for them. Demo packs document panel changes as
manual, optional steps. Panel/layout support is a post-MVP capability requiring a new
schema version and an ADR.

### Reasoning

- Shipping an unreliable layout mutation would undermine the safety promise.
- Executing layout scripts contradicts ADR-006.
- Synthesizing containment trees is brittle across Plasma minors and hard to verify.
- Restarting plasmashell is disruptive and increases the failure surface.
- Scope discipline: V0.1 must prove the transaction model on stable resource types first.

### Tradeoffs

- Demo packs are less visually complete; Glass's floating panels are manual.
- Some users will consider panel layout the most important customization.
- A future implementation will be more complex than if it had been included early.

### Consequences

- MVP acceptance criteria exclude panel/layout operations.
- `PACK_SPEC.md` reserves `panel`/`layout` for schema v2 and rejects them in v1.
- README, MVP, PRD, and demo READMEs state the limitation explicitly.
- Revisiting requires an ADR covering containment synthesis, restart policy, and rollback.

---

## ADR-012 — Compose appearance instead of applying Look-and-Feel packages

- **Status:** **Superseded by [ADR-019](#adr-019--global-themes-as-decomposed-plans)**
- **Date:** 2026-10-18
- **Superseded on:** 2026-10-18 after live capability testing

> **Supersession note.** Testing showed `plasma-apply-lookandfeel -a` does **not** reset
> the panel layout (that requires the separate `--resetLayout` flag), does **not** execute
> package scripts in this case, and expresses its changes as a readable `contents/defaults`
> file that decomposes into operations Morphix already supports. The "compose" half of this
> ADR survives; the "do not support global themes" half is reversed.

### Context

KDE supports global themes ("Look-and-Feel" packages) that bundle color scheme, Plasma
style, decorations, icons, cursors, and sometimes defaults/scripts. Applying one is
all-or-nothing and can trigger package-provided defaults or scripts. Morphix needs
per-resource preview, verification, and rollback.

### Decision

V0.1 does not apply Look-and-Feel packages. A pack composes appearance from individual
resources: color scheme, icons, cursors, Plasma style, window decoration, wallpaper, and
allowlisted settings.

### Reasoning

- Each resource maps to a known target, verifiable state, and reverse operation.
- A single "apply global theme" call hides what changed and cannot be partially reversed.
- Avoids package-provided scripts/defaults, consistent with ADR-006.
- Users see exactly which pieces will change.

### Tradeoffs

- Packs cannot reuse existing global themes as a unit.
- Authors must assemble resources themselves.
- Some KDE defaults that a global theme would set are not replicated.

### Consequences

- `PACK_SPEC.md` has no `look_and_feel` asset kind.
- A future global-theme import would need an ADR and a way to decompose the package into
  typed operations.
- The UI presents appearance as a set of independent, reviewable changes.

---

## ADR-013 — Separate `morphix-core` crate from the Tauri shell

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

The proposed repository structure put all Rust modules under `src-tauri/src`. That makes
the core depend on the application shell and complicates headless testing. NFR-8 requires
the core to build and test without Tauri, a display server, or a Plasma session.

### Decision

Use a Cargo workspace with `crates/morphix-core` (all domain logic) and `src-tauri`
(commands, state, events only). `morphix-core` must not depend on Tauri, WRY, or WebKitGTK.

### Reasoning

- Enforces the dependency direction mechanically, not by convention.
- Enables fast headless tests and future CLI use of the same core.
- Prevents GUI concerns from leaking into planning or transaction code.
- Keeps `src-tauri` small and auditable.

### Tradeoffs

- Slightly more workspace setup than a single crate.
- Cross-crate refactors require updating both manifests.
- Contributors must understand which crate owns a change.

### Consequences

- `tests/architecture.rs` enforces module edges and the no-Tauri rule.
- CI can run core tests without GUI system dependencies.
- The Tauri shell contains no domain logic; commands translate and delegate.

---

## ADR-014 — Strict schema versioning

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Pack manifests will evolve. Ignoring unknown fields is a common forward-compatibility
trick, but in a security-sensitive system it lets a pack smuggle semantics that an older
engine silently drops or misinterprets.

### Decision

Schema v1 rejects unknown and reserved fields. `schema_version` must be exactly `1`.
Forward compatibility is handled by explicit version bumps, never by silent tolerance.
`deny_unknown_fields` is mandatory on manifest models, and reserved names (`layout`,
`panel`, `scripts`, `hooks`, `permissions`) are always invalid in v1.

### Reasoning

- A pack that validates must mean exactly what its author intended on that engine.
- Silent field dropping can turn a "preview" into a different change than declared.
- Version bumps create a clear compatibility contract and test target.
- It simplifies the validator: no dead or ambiguous fields.

### Tradeoffs

- A newer pack cannot be partially used by an older app; it is refused.
- Schema evolution needs coordination and a migration story for packs.
- Authors must re-validate packs against the app version they target.

### Consequences

- `E_SCHEMA_VERSION_UNSUPPORTED` and `E_UNKNOWN_FIELD` are hard errors.
- New capabilities require schema v2 and an ADR, not optional fields.
- Fuzz tests assert no panic on unknown/duplicate keys.

---

## ADR-015 — Wallpaper via a constrained Plasma scripting call

- **Status:** Accepted; prototype-gated
- **Date:** 2026-10-18

### Context

Plasma 6 exposes no stable public DBus method for setting the desktop background. The
practical interface is `org.kde.plasmashell` `/PlasmaShell` `evaluateScript`, which executes
JavaScript in plasmashell. ADR-006 forbids arbitrary script execution, but here the script is
*engine-generated*, not pack-provided.

### Decision

Allow exactly one internal scripting use: a fixed, Rust-generated wallpaper script whose
only interpolated value is a validated absolute file URL encoded as a JSON string literal.
The pack supplies an asset key, never script text. Capability availability is probe-gated;
if read-back or invocation is unreliable, the adapter reports `Degraded` and plans emit a
manual step instead of mutating.

### Reasoning

- Wallpaper is a core part of the demo packs and the product's visual promise.
- Restricting the script to a constant template with one escaped value keeps pack input out
  of the script engine.
- Snapshotting the prior wallpaper descriptor makes reversal possible.
- A probe gate prevents pretending to support versions where it fails.

### Tradeoffs

- Depends on an internal Plasma interface that may change across minor versions.
- Read-back is not guaranteed; verification may be weaker than for config files.
- Security review must treat the script generator as sensitive code.
- Reviewers may see any scripting as a violation; the distinction must be explicit.

### Consequences

- `desktop/kde/wallpaper.rs` is the only script generator; golden tests pin its shape.
- SECURITY lists the script boundary and injection tests.
- If the Phase 6 spike fails on the declared matrix, the fallback (install asset + manual
  step) ships and this ADR is updated; the feature is not silently half-working.
- The script must never accept a second interpolated value without a new ADR.

---

## ADR-016 — No root, no daemon, no privileged helper in V1

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Desktop customization normally only needs user-scope files. Some tools ask for `sudo` to
install themes system-wide or to restart services. A privileged component would drastically
increase the security and maintenance burden.

### Decision

V0.1 runs as the user, refuses to mutate when `geteuid() == 0`, writes only under the user's
home, and includes no daemon, setuid binary, polkit action, or privileged helper. Normal
customization must never require elevation.

### Reasoning

- User-scope changes cover every V0.1 capability.
- No privileged surface means no privilege-escalation path through Morphix.
- Avoids a long-running daemon and its lifecycle/security complexity.
- Matches the local-first, least-privilege product stance.

### Tradeoffs

- Cannot install system-wide themes or packages.
- Users who expect a system-wide change get a refusal with an explanation.
- Future capabilities needing privilege will require a separate design.

### Consequences

- Refusal with `E_RUNNING_AS_ROOT` is tested.
- No `sudo`/`pkexec` invocations; CI static checks enforce this.
- A future helper must be a separate, narrow, typed broker with its own ADR and threat
  model; the app itself will never run as root.

---

## ADR-017 — Licensing: code and pack assets

- **Status:** Accepted (pre-release; LICENSE files added in Phase 0)
- **Date:** 2026-10-18

### Context

Morphix ships source code and first-party demo pack assets. The packs must be freely
redistributable and must not contain proprietary brand assets.

### Decision

Source code: dual-licensed `MIT OR Apache-2.0`. First-party pack assets: `CC BY 4.0`, with
provenance and attribution in each pack's `ATTRIBUTION.md`. Third-party assets are allowed
only if their license is compatible and recorded. No Apple, Microsoft, or other proprietary
logos, icons, fonts, wallpapers, or theme files are shipped.

### Reasoning

- Rust ecosystem convention favors MIT/Apache-2.0 dual licensing.
- CC BY 4.0 is appropriate for creative assets and requires the attribution the project
  wants anyway.
- Clear provenance reduces legal risk and supports the trust narrative.
- A permissive code license encourages adapter and pack ecosystem growth.

### Tradeoffs

- CC BY requires attribution when assets are reused; pack authors must comply.
- Dual licensing adds two license files and some contributor questions.
- SPDX validation in the manifest constrains license strings.

### Consequences

- `LICENSE-MIT`, `LICENSE-APACHE`, and per-pack `ATTRIBUTION.md` are required.
- The asset audit in Phase 9 checks provenance and rejects brand assets.
- `license` in `pack.toml` must be a valid SPDX expression.
- Changing the license later requires an ADR and contributor consideration.

---

## ADR-018 — Panel, widget and desktop-behaviour customization in V0.1

- **Status:** Accepted
- **Date:** 2026-10-18
- **Supersedes:** ADR-011

### Context

ADR-011 excluded panels and layouts on the untested belief that the only options were
forbidden layout scripts or fragile containment editing plus a shell restart. Live testing
on Plasma 6.8 disproved that.

### Decision

V0.1 supports panel properties (edge, height, alignment, floating, hiding), panel creation
and removal, allowlisted widgets, and allowlisted desktop behaviour (virtual desktops, hot
corners, animation speed, window rules, switcher layouts), all through supported APIs with
read-back verification and reverse operations.

### Reasoning (evidence)

- `panels()[0]` exposes `location/height/alignment/floating/hiding/opacity` and accepted
  every writable one; read-back confirmed each change and each restore.
- `new Panel()` created id 46, `remove()` deleted it, and only inert
  `[PlasmaViews][Panel 46]` sections remained, which the engine now cleans.
- `addWidget`/`configureWidget`/`remove` round-tripped on panel 23 with the original seven
  widgets and `AppletOrder` restored.
- `VirtualDesktopManager` created and removed a desktop with `kwinrc` converging correctly.
- `[ElectricBorders]` round-tripped with `KWin.reconfigure` and a byte-identical `kwinrc`.

### Tradeoffs

- Panel opacity is **not** supported: the scripting property is silently read-only, and no
  alternative was proven. Packs requesting it are refused.
- Widget removal is asynchronous, so verification must poll.
- Panel persistence lags ~15 s, requiring a quiesce step.
- Panel and widget identity is not stable: restored widgets receive new ids.

### Consequences

- Six capabilities added to the closed set (`panel.properties`, `panel.widgets`,
  `panel.create`, `settings.desktop_behavior`, plus the L&F and font capabilities).
- New operation kinds, reverse semantics, residue cleanup and error codes.
- Glass-style packs can now ship floating panels instead of manual steps.
- Multi-screen behaviour remains unvalidated and must be tested before release.

---

## ADR-019 — Global themes as decomposed plans

- **Status:** Accepted
- **Date:** 2026-10-18
- **Supersedes:** ADR-012

### Context

ADR-012 excluded Look-and-Feel packages to preserve per-resource preview and rollback.
Testing showed the exclusion was unnecessary and its premise partly wrong.

### Decision

V0.1 accepts a global theme as pack input, but applies it as a **decomposed plan**: parse
the package's `contents/defaults`, expand every declared key into a primitive operation,
snapshot each, apply, verify per component, and roll back per component. The
`LookAndFeelPackage` marker is written as one additional operation. `--resetLayout` is
never passed.

### Reasoning (evidence)

- `-a org.kde.breezedark.desktop` left the panel, its id and its seven widgets untouched.
- The tool's actual effect was to delete user overrides and write one marker
  (`LookAndFeelPackage`), so the change set is small and describable.
- `contents/defaults` is a plain KConfig (`[kdeglobals][General] ColorScheme=Vapor`), so it
  maps one-to-one onto operations Morphix already has.
- Restoring required including `~/.config/kdedefaults/`, which the tool rewrites.

### Tradeoffs

- Decomposition must match the tool's semantics, including deleted overrides and
  `kdedefaults`; a naive re-apply diverges.
- Detection is heuristic: `/etc/xdg/kdeglobals` supplies a system-wide marker, so the marker
  alone over-reports. Mixed configurations are reported as "custom".
- A package may declare components that are not installed (for example
  `widgetStyle=Windows`), which must be refused rather than silently falling back.

### Consequences

- `appearance.look_and_feel` capability plus `[appearance.look_and_feel]` manifest section.
- Snapshot scope grows to include `kdedefaults/` and deleted-override undo data.
- Demo packs may reference an installed global theme as a starting point.

---

## ADR-020 — Never restart the desktop shell

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Some appearance changes are only picked up at plasmashell start (icon theme, some font
keys). The obvious remedy is to restart the shell. Testing showed that repeated restarts
hit systemd's start-rate limit:

```
Job for plasma-plasmashell.service failed because start of the service was attempted too often.
```

which left the session with **no panel at all** until `reset-failed` plus a manual start.

### Decision

Morphix never restarts plasmashell, KWin, or the session, and never invokes `systemctl` or
`--replace`. Every operation declares an apply class (`live`, `reconfigure`,
`restart_required`, `app_restart`); `restart_required` changes are written to config,
verified at state level, and reported as taking effect on the next login.

### Reasoning

- The engine's first duty is not to break the desktop; a restart is the one action that can.
- Rate limiting is outside our control and is triggered by legitimate-looking sequences.
- Config-only application is still reversible, so the safety model is preserved.
- Users can restart at a time of their choosing.

### Tradeoffs

- Icon theme changes appear only after logout/login, which may surprise users.
- The UI must communicate apply classes clearly to avoid a "nothing happened" reaction.

### Consequences

- `restart_required` and `app_restart` in the plan model and UI.
- Architecture test forbidding `systemctl`/`plasmashell` argv.
- `E_RESTART_REQUIRED` for attempts to verify a non-live change as if it were live.

---

## ADR-021 — Plasma scripting is a trusted-generator boundary

- **Status:** Accepted
- **Date:** 2026-10-18
- **Relates to:** ADR-006, ADR-015

### Context

Panels, widgets and wallpaper require `org.kde.plasmashell` `evaluateScript`. Testing the
context showed it is powerful: `new ConfigFile(path, false)` plus `writeEntry`/`deleteEntry`
wrote `~/.config/kdeglobals` and created new files, and `ConfigFile` has **no** path
restriction. It also has sharp edges: `readConfig` takes `(key, default)` from
`currentConfigGroup`, and the wrong call silently returns the key name and writes garbage.

### Decision

Scripts are treated as trusted code generated by Rust, never as a sandbox, and never as a
place where pack text is interpolated. Only fixed templates are emitted; pack input reaches
them solely as closed-set enum identifiers or engine-derived, JSON-escaped absolute paths.
`ConfigFile` is never constructed by the engine, and an architecture/golden test enforces
that.

### Reasoning

- The interpreter cannot be made safe from inside; the generator can be made boring.
- Fixed templates make the script text reviewable and snapshot-testable.
- Enum-only injection removes the quoting/injection surface entirely.

### Tradeoffs

- New panel/widget behaviours require new templates and tests, not ad-hoc scripts.
- Read-back templates are separate from write templates, doubling the golden files.

### Consequences

- `desktop::kde::scripting` owns the only script builder.
- T-29 and T-30 in SECURITY.md.
- Any script change is a reviewable diff plus a golden-file update.

---

## ADR-022 — Verify live state and effective configuration

- **Status:** Accepted
- **Date:** 2026-10-18

### Context

Two measurement problems appeared in testing:

1. `~/.config/kdedefaults` is an `XDG_CONFIG_DIRS` entry, so the *effective* value differs
   from the user file (`plasmarc` reported `Vapor` with no `~/.config/plasmarc` at all).
2. Screenshot pixel comparison was unusable: full-screen RMSE was dominated by a running
   browser (0.18 between two states of the same theme), while a cropped panel strip was
   pixel-stable (RMSE 0 over 4 s).

### Decision

Verification is defined per resource and uses the most authoritative channel available:
live daemon state first (`supportInformation`, `/Effects`, `wallpaper(u)`, panel scripts,
`kglobalaccel.shortcut()`), effective configuration second (user file merged with
`kdedefaults` and `/etc/xdg`), pixels only as supplementary evidence on a static crop.

### Reasoning

- Live state is what the user sees and cannot be spoofed by a stale file.
- Effective-value reads make "restore to absent" correct instead of creating overrides.
- Pixel diffing on a live desktop is not reproducible enough to gate a transaction.

### Tradeoffs

- Each capability needs its own verifier, which is more code than a generic diff.
- Some parameters (effect strengths, window rules) still have no live channel and can only
  be state-verified; the UI must say so.

### Consequences

- `ResourceState` distinguishes `Absent` from a value.
- `kdedefaults/` is part of snapshot scope.
- TESTING.md forbids full-screen pixel assertions as a pass criterion.

---

## ADR-023 — Allowlist tables live in the `core` module

- **Status:** Accepted
- **Date:** 2026-10-21

### Context

ARCHITECTURE.md §4 originally placed the closed allowlist tables (config targets and
keys, KWin effects, shortcut components/actions, panel and widget enums, look-and-feel
components) in `permissions/allowlist.rs`. P2-08 could not build them there:

1. `packs::validator` must reject a declaration against the same tables the grant engine
   uses. Two copies of a security table drift, and a drift is a security bug in whichever
   direction it happens.
2. ARCHITECTURE.md §5 forbids `packs` from depending on `permissions`.

The only module both may depend on is `core`.

### Decision

The tables live in `core::allowlist` as pure data with no I/O. `packs::validator` reads
them to reject declarations; `permissions::resolve` reads them to compute grants.
`permissions/allowlist.rs` does not exist.

### Reasoning

- One table, one source of truth. A key accepted by the validator is exactly a key the
  grant engine knows how to authorize.
- `core` is already the shared vocabulary module (`Capability`, `ConfigTarget` values are
  meaningful to both a validator and a resolver), so the tables fit its role.
- The tables are compile-time constants; nothing is added to `core`'s dependency set.

### Tradeoffs

- `core` grows beyond "IDs, versions, capabilities, resource types" into policy data. It
  stays policy *data*; no behavior, no I/O, no decisions.
- `permissions` is thin until P3-06/P3-12 add `resolve` and `consent`.
- A reader looking for "the allowlist" in the module named `permissions` will not find it;
  the doc comment at the top of `core/allowlist.rs` explains why.

### Consequences

- ARCHITECTURE.md §4 module tree and component table updated.
- A drift test must assert that the validator and the resolver consult the same functions
  rather than duplicated tables.
- Future allowlist additions go in `core::allowlist` only.

---

## ADR-024 — Acceptance criteria follow the phase that supplies their dependency

- **Status:** Accepted
- **Date:** 2026-10-21

### Context

Several acceptance criteria are named by the exit criteria of a phase that cannot
demonstrate them:

- Phase 2 names AC-05 (catalog lists four bundled packs), AC-06 (pack detail) and AC-08
  (rendered change set). AC-05 needs the four first-party packs (Phase 9); AC-06 and AC-08
  need the Discover / Pack Detail routes (Phase 8).
- Phase 3 names AC-09 (plan content), AC-10 (plan purity), AC-21 (refusal) and AC-24
  (wallpaper). AC-09 and AC-10 are written against "a bundled pack", which is Phase 9.
  AC-24 asserts an apply-time read-back through `org.kde.PlasmaShell.wallpaper(u)`, which
  needs the transaction engine (Phase 5) and the KDE adapter (Phase 6).
- Phase 3 must read live KDE pre-state to build a real plan, but the adapter read methods
  (`resolve_target`, `read_state`, `validate_value`) were deliberately left off the
  `DesktopAdapter` trait in Phase 1 and are scheduled in Phase 6.

PHASES.md rule 5 forbids starting a phase while the previous phase's exit criteria are
unmet unless an ADR records the exception.

### Decision

An acceptance criterion is demonstrated in the phase that supplies its dependency. The
earlier phase records the criterion as **deferred**, names the phase that will demonstrate
it, and never marks it met. Within its own phase, the observable half is demonstrated with
test fixtures.

Concretely:

- **Phase 2** exits with AC-05/AC-06/AC-08 deferred to Phases 8–9, and meets AC-07, AC-18
  and AC-19.
- **Phase 3** implements the planner as a pure function over injected pre-state. Tests
  supply a fake adapter and a recording state, and satisfy the planner-observable halves of
  AC-09, AC-10, AC-21 and AC-24 using fixture packs. The bundled-pack half of AC-09/AC-10
  is re-verified in Phase 9; the apply-time half of AC-24 in Phases 5–6.
- The `create_pack_plan` command returns `E_ADAPTER_UNAVAILABLE` until Phase 6 injects the
  real KDE read path. It never returns an empty or optimistic plan.

### Reasoning

- Building the UI or the KDE read path early would collapse two phase boundaries and make
  the remaining plan meaningless.
- Declaring a criterion met from a test that mocks away its dependency is a false pass; it
  is the failure mode this repository's testing rules exist to prevent.
- A typed refusal is honest to the user: a specific error code, not a plan that looks valid
  and applies nothing.

### Tradeoffs

- Between Phase 3 and Phase 6 the shipped app cannot produce a plan from a real session.
  This is a visible capability gap for two phases, not a hidden one.
- Phase 3's exit criteria are met by planner-level evidence, so Phases 5, 6 and 9 must
  re-verify their apply-time and bundled-pack halves rather than assuming them.
- The deferral list must be maintained in three places (PHASES.md, TASKS.md, README.md) or
  it becomes untrue.

### Consequences

- PHASES.md Phase 2 and Phase 3 exit criteria are annotated with the deferral.
- TASKS.md records the deferred criteria against the phase that postpones them.
- `E_ADAPTER_UNAVAILABLE` from `create_pack_plan` is a tested refusal path.
- Phase 6 must remove that refusal path in the same change that adds the adapter reads.

---

## Adding an ADR

1. Copy the format above into a new numbered section (next free number).
2. State the context factually, including constraints and what is *not* known.
3. State the decision in one or two sentences.
4. Record reasoning and tradeoffs honestly; an ADR with no downsides is usually incomplete.
5. List consequences, including documents and code that must change.
6. Update the index and any documents affected in the same change set.
7. ADRs are immutable once accepted. To change a decision, add a new ADR with
   `Supersedes: ADR-xxx` and update the old record's status to `Superseded by ADR-yyy`.

### Decisions that require a new ADR

- Adding a desktop adapter or changing the adapter contract.
- Adding a capability, operation kind, or writable target.
- Any change to the transaction, snapshot, or rollback model.
- Adding network access, accounts, telemetry, or a registry.
- Adding a privileged helper or running any component as root.
- Relaxing path validation, symlink policy, or script restrictions.
- Changing the pack schema version or dependency semantics.
- Changing the database engine or retention policy.
