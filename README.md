# Morphix

**Safe, reversible customization for the Linux desktop.**

Morphix is an open-source desktop customization platform for KDE Plasma 6. Users can
browse a curated catalog of customization packs, inspect exactly what a pack will change,
preview the change plan, apply it transactionally, and roll the entire change back —
completely, at any time, without reinstalling their desktop.

Morphix is an original project. It is conceptually inspired by the *idea* of a
customization marketplace with safe, inspectable mods, but it is not a port of Windhawk,
shares no code with it, and does not target Windows.

> ## Project status: Phase 0 in progress
>
> **There is no application functionality yet.** The Phase 0 foundation is partly in
> place: the Cargo workspace, the `morphix-core` module skeleton, the error taxonomy
> (73 stable codes with registry drift tests), the test that enforces the module
> boundaries, a Tauri 2 shell that opens the application window and answers `get_app_info`,
> and the Vite/React shell with the eight placeholder routes. CI and the documentation
> checks are still being added. No detection, pack, planning, snapshot, transaction, or
> adapter code exists. Everything described below is the intended V0.1/V1 behavior, not a
> claim about working software. See [PHASE0.md](PHASE0.md), [PHASES.md](PHASES.md), and
> [TASKS.md](TASKS.md) for the implementation plan, and [DECISIONS.md](DECISIONS.md) for
> the reasoning behind the scope.

---

## The problem

Customizing a Linux desktop is powerful but fragile.

- Theme installers are usually shell scripts. Users are asked to `curl | bash` a script
  they cannot audit, which writes into their home directory with no record of what it
  changed.
- Installing a theme often means manually copying directories into undocumented XDG
  locations, editing KDE config files, and restarting Plasma components.
- Uninstalling is guesswork. Config keys are overwritten without a backup, an old theme
  directory is deleted, and the user's previous look is gone.
- The result of an installation is usually invisible until after it happens, and failures
  leave the desktop in a half-applied state.
- There is no trustworthy place to discover desktop customizations, and no way for a
  creator to ship a customization that a cautious user can verify before running.

## The product vision

> The modding and customization platform for the Linux desktop.

Discover a customization, see exactly what it will change, preview it, apply it safely,
and undo it completely. The long-term vision is a platform with a creator ecosystem and
adapter support for more desktops. The V1 scope is deliberately narrow: **KDE Plasma 6
only**, executed exceptionally well.

## How this differs from a theme installer

| Typical theme installer | Morphix |
| --- | --- |
| Runs an opaque `install.sh` | Never executes pack-provided code; packs are declarative TOML |
| Writes files with no record | Builds a typed change plan before touching anything |
| No preview | Shows every file, config key, and old → new value before applying |
| No backup | Snapshots every affected resource before the first write |
| Uninstall is the author's problem | Engine-owned rollback, verified and recorded |
| Failure leaves a half-applied desktop | Transaction with reverse-order rollback and crash recovery |
| Trust the author | Capability declarations, path confinement, checksums, explicit consent |
| Hidden system-wide changes | User-scope only, no root, no silent privilege escalation |

## Core MVP (V0.1)

1. Detect the Linux distribution.
2. Detect the KDE Plasma version and session type.
3. Detect the system package manager (informational).
4. Report whether the machine is supported, with reasons.
5. Browse bundled customization packs.
6. View pack screenshots, descriptions, and license/attribution.
7. View compatibility information for a pack.
8. View exactly which resources a pack will modify.
9. Preview a resolved change plan with old → new values and a file footprint.
10. Create a verified snapshot before every modification.
11. Apply a customization transactionally.
12. Roll back an applied customization.
13. Restore an older snapshot.
14. Maintain local change history.
15. Install a customization pack from a local directory.
16. Validate pack manifests and reject unsafe or unsupported input.
17. Enforce a permission/capability model with explicit user consent.
18. Refuse unsupported or unsafe operations with an actionable explanation.

Supported resource kinds in V0.1: color schemes, icon themes, cursor themes, Plasma
styles, window decorations, wallpapers, global themes (applied as decomposed plans),
fonts, panel properties, panel creation, widgets, allowlisted KDE global settings, KWin
effects, keyboard shortcuts, virtual desktops, hot corners, animation speed and window
rules. Every one of these was exercised on a live Plasma 6.8 session; see
[CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) and [PROTOTYPE_RESULTS.md](PROTOTYPE_RESULTS.md).

The user's panel **layout is preserved**, not replaced: applying a theme or pack never
writes a new layout and never passes `--resetLayout`. The capabilities that remain
unsupported are panel opacity and cursor size, because no reliable mechanism was found;
packs requesting them are refused rather than silently ignored. Full-screen layout
replacement is out of scope (ADR-018, ADR-019).

## Supported environment

- **Desktop:** KDE Plasma 6.x (`>=6.0, <7.0`). Plasma 5, GNOME, Hyprland, XFCE, Cinnamon,
  and other desktops are explicitly unsupported by V1.
- **Session:** Wayland validated; X11 accepted but not yet validated on Plasma 6.
- **Distribution:** any Linux distribution shipping Plasma 6. The distribution is
  detected and displayed, but is not a hard gate; Morphix reports which distributions
  have been validated for a release instead of claiming universal compatibility.
- **System scope:** user session only. Morphix does not require root and does not modify
  system-wide paths in V0.1.
- **Network:** not required. The V0.1 catalog is bundled; pack installation is from a
  local directory.

## Architecture summary

```
React + TypeScript UI
        │  (typed IPC, no filesystem or process access)
        ▼
Tauri 2 shell (thin: commands, events, state)
        ▼
morphix-core (Rust): domain, validation, planning, transactions
        ├── Permission / capability engine
        ├── Change planner
        ├── Snapshot engine
        ├── Transaction + rollback engine
        ├── SQLite history repository
        └── KDE adapter ──► KDE config files / XDG data dirs / DBus
```

All system-sensitive work lives in Rust. The frontend never mutates configuration; it
requests narrow, typed operations and renders the plans and results it receives. See
[ARCHITECTURE.md](ARCHITECTURE.md).

## Safety philosophy

1. **Safety before customization power.** A customization that cannot be reversed is not
   offered.
2. **Every change is planned, snapshotted, applied, verified, and committed — or rolled
   back.** No direct writes from UI code, ever.
3. **Packs are untrusted input.** A pack declares typed operations and capabilities; it
   never supplies shell commands, absolute paths, or scripts.
4. **Show before you change.** The user sees the resolved plan, including values that
   will be overwritten, before consenting.
5. **Least privilege.** User scope, no root, no silent `sudo`, no system-wide writes.
6. **No invented compatibility.** Unsupported or unverified environments are reported as
   such, not worked around silently.

## Repository documents

| Document | Purpose |
| --- | --- |
| [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) | Every capability with its measured state and final approach |
| [PROTOTYPE_RESULTS.md](PROTOTYPE_RESULTS.md) | Per-experiment log: commands, evidence, problems, recommendation |
| [PRD.md](PRD.md) | Product requirements, users, journeys, metrics, risks |
| [MVP.md](MVP.md) | Exact V0.1 scope with testable acceptance criteria |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module boundaries, data flow, dependency rules |
| [PACK_SPEC.md](PACK_SPEC.md) | Declarative pack format (schema v1) and validation |
| [SECURITY.md](SECURITY.md) | Threat model, trust boundaries, mitigations |
| [TRANSACTIONS.md](TRANSACTIONS.md) | Operation model, journal, rollback, crash recovery |
| [DATABASE.md](DATABASE.md) | SQLite schema, relationships, indexes |
| [UI_SPEC.md](UI_SPEC.md) | Screens, states, copy and failure presentation |
| [TESTING.md](TESTING.md) | Test strategy, including failure-injection tests |
| [PHASES.md](PHASES.md) | Implementation phases with exit criteria |
| [TASKS.md](TASKS.md) | Implementation-ready task checklist |
| [PHASE0.md](PHASE0.md) | Execution plan for Phase 0: stages, gates, environment |
| [DECISIONS.md](DECISIONS.md) | Architecture Decision Records |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to develop, test, and propose changes |
| [AGENTS.md](AGENTS.md) | Rules for AI coding agents working in this repository |

## Repository layout

```
morphix/
├── *.md                      # the documents listed above
├── Cargo.toml                # workspace
├── crates/morphix-core/      # all domain, planning, transaction, adapter logic
├── src-tauri/                # Tauri 2 shell: commands, events, app state
├── src/                      # React + TypeScript UI (Vite)
├── packs/                    # four first-party demo packs (declarative only)
├── tests/                    # cross-crate integration + malicious-pack fixtures
└── resources/                # bundled pack resources shipped with the app
```

`crates/morphix-core` holds the module skeleton and the error taxonomy; `src-tauri` holds
the Phase 0 shell (`get_app_info` and a placeholder window); `src/` holds the React shell
with placeholder routes. The remaining directories appear as their phases land.
[PHASES.md](PHASES.md) defines the implementation order and [TASKS.md](TASKS.md) the task
checklist.

## Roadmap

V0.1 (MVP) is defined in [MVP.md](MVP.md). Post-MVP work — pack registry, pack signing,
additional desktop adapters, and pack removal — is deliberately unscheduled and listed as
possibilities in [PRD.md](PRD.md) and [DECISIONS.md](DECISIONS.md). Nothing in those
sections is a commitment.

## License

Source code is dual-licensed under `MIT OR Apache-2.0` — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE). First-party demo pack assets are `CC BY 4.0`. Every
bundled asset must be original or openly licensed, with attribution recorded next to the
pack. No Apple, Microsoft, or other proprietary assets, logos, fonts, icons, or wallpapers
are shipped. See [DECISIONS.md](DECISIONS.md) (ADR-017).
