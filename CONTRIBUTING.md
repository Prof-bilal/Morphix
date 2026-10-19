# Contributing to Morphix

Morphix is a safe, reversible customization platform for KDE Plasma 6. Contributions are
welcome, but the project is intentionally conservative: a feature that cannot be previewed,
verified, and rolled back is not ready.

## Repository status

**Phase 0 is in progress.** The repository contains the specifications, a task plan, the
Cargo workspace, the `morphix-core` module skeleton, the error taxonomy, the Tauri 2 shell,
and the React shell. There is no application functionality yet. The Rust and frontend
commands below run today; the documentation command is **planned** until P0-09 lands (see
[PHASE0.md](PHASE0.md), [PHASES.md](PHASES.md), and [TASKS.md](TASKS.md)). Do not add
instructions to this file claiming a command works before it does.

## Read before writing

In order:

1. [README.md](README.md) - what the product is and is not.
2. [PRD.md](PRD.md) - users, principles, requirements, non-goals.
3. [MVP.md](MVP.md) - the exact V0.1 scope and acceptance criteria.
4. [ARCHITECTURE.md](ARCHITECTURE.md) - module boundaries and dependency rules.
5. [SECURITY.md](SECURITY.md) - threat model and security gates.
6. The document specific to your change: [PACK_SPEC.md](PACK_SPEC.md),
   [TRANSACTIONS.md](TRANSACTIONS.md), [DATABASE.md](DATABASE.md),
   [TESTING.md](TESTING.md).
7. [DECISIONS.md](DECISIONS.md) - why the architecture is what it is.

If a proposed change contradicts a document, the document wins until an ADR changes it.

## Scope rules

- **KDE Plasma 6 only.** Do not add GNOME, Hyprland, XFCE, Cinnamon, or generic desktop
  abstractions.
- **No panel or Plasma layout changes in V0.1** (ADR-011).
- **No network, accounts, telemetry, registry, or auto-update** in V1 (ADR-008).
- **No root, daemon, setuid, or privileged helper** (ADR-016).
- **No pack-provided scripts, shell commands, or executable content** (ADR-006).
- **No new capability, operation kind, or writable target** without an ADR, tests, and
  documentation updates.
- Do not expand scope because a feature "would be nice". File an issue instead.

## Development environment (planned)

Prerequisites for building the app once Phase 0 lands:

- Linux with a KDE Plasma 6 session for end-to-end testing (not required for core tests).
- Rust: the version pinned in `rust-toolchain.toml` (installed via `rustup`).
- Node.js (LTS) and npm for the frontend.
- Tauri 2 system dependencies for your distribution: WebKitGTK 4.1 development headers,
  GTK 3 headers, librsvg, OpenSSL, and a C toolchain. Exact package names differ by
  distribution; consult your distribution's Tauri prerequisites.
- SQLite is compiled in via `rusqlite`'s bundled feature; no system SQLite is required.

| Task | Command | Available |
| --- | --- | --- |
| Rust format check | `cargo fmt --all --check` | yes |
| Rust lints | `cargo clippy --workspace --all-targets -- -D warnings` | yes |
| Rust tests | `cargo test --workspace` | yes |
| Build the desktop shell | `cargo build -p morphix-app` | yes |
| Install frontend dependencies | `npm ci` | yes |
| Run the web UI only | `npm run dev` | yes |
| Run the desktop app | `npm run tauri dev` | yes |
| Typecheck | `npm run typecheck` | yes |
| Frontend tests | `npm run test -- --run` | yes |
| Architecture boundary test | `cargo test -p morphix-core --test architecture` | yes |
| Docs checks | `python3 tests/check_docs.py` | planned (P0-09) |
| KDE integration tests (VM) | `MORPHIX_KDE_INTEGRATION=1 cargo test -p morphix-core --test kde_integration -- --ignored` | planned (Phase 6) |

Never run integration tests against your live session. Use a disposable VM or a throwaway
user account, and always isolate state:

```bash
export HOME="$(mktemp -d)"
export MORPHIX_DATA_DIR="$HOME/.local/share/morphix"
export MORPHIX_STATE_DIR="$HOME/.local/state/morphix"
```

## Architecture orientation

```
React UI  ->  Tauri commands  ->  morphix-core  ->  transaction engine
                                                  ->  snapshots
                                                  ->  KDE adapter -> files / DBus
```

- `crates/morphix-core` owns all domain logic and must build without Tauri or a display.
- `src-tauri` owns commands, DTOs, events, and app state. No domain logic.
- `src/` owns presentation. No filesystem, DBus, or configuration knowledge.
- `desktop::kde` is the only module allowed to know KDE paths or call KDE DBus interfaces.
- `storage` is the only module allowed to contain SQL.
- `transactions` is the only module allowed to mutate user state.

Dependency directions are enforced by `tests/architecture.rs`. If you need a new edge,
change the architecture and the ADR first.

## Adding a Rust module

1. Decide which module owns the concern, based on [ARCHITECTURE.md](ARCHITECTURE.md) §4-5.
2. Create a directory module with a focused responsibility, not a grab bag.
3. Define typed inputs/outputs and a `thiserror` error that maps to a stable `ErrorCode`.
4. Keep I/O behind a trait if it needs to be faked in tests (`FileSystem`, `KdeBus`,
   `Clock`, `CommandRunner`).
5. Add unit tests beside the module and integration tests if it crosses modules.
6. Add failure tests for every error branch that can affect state.
7. Update the module tree in ARCHITECTURE if you add a module; update the ADR index if the
   change is architectural.
8. Keep the file under roughly 600 non-test lines. Split by responsibility, not by line
   count.

Do not:

- add `utils.rs`, `helpers.rs`, or `common.rs` catch-alls;
- use `unwrap()`/`expect()` on fallible input or I/O in core code (tests may use `expect`
  with a message);
- return `anyhow::Error` from public core APIs;
- add `unsafe` (forbidden in `morphix-core`; an exception requires an ADR);
- spawn processes outside the allowlisted probe module;
- hide an error by logging and continuing when the caller needs to know.

## Modifying the UI

1. Add or change the typed DTO in Rust first, then regenerate TS types (`ts-rs`); never
   hand-write a mismatched interface.
2. Call IPC only through `src/api/client.ts`. Components use hooks, not `invoke`.
3. Render pack-provided text as text. No `dangerouslySetInnerHTML`, no remote content.
4. Make failure states explicit: rollback succeeded, rollback failed, and recovery required
   must be visually distinct and never auto-dismissed as success.
5. Keep the Apply button disabled until the displayed plan hash matches the plan being
   applied and every capability is acknowledged.
6. Test keyboard-only flow, focus management, and dark/light contrast.
7. Add component tests for the happy path and at least one failure path.

## Creating a pack

Read [PACK_SPEC.md](PACK_SPEC.md) fully. Minimum requirements:

- Original or compatibly licensed assets; `ATTRIBUTION.md` with provenance.
- No Apple/Microsoft/other proprietary logos, icons, fonts, wallpapers, or theme files.
- A truthful `[compatibility].plasma` range; do not claim versions you have not tested.
- Request only the capabilities you use, with allowlisted keys/effects/shortcuts.
- Include previews, README, and `checksums.sha256` generated after assets are final.
- Test on a clean Plasma 6 user: apply, restart Plasma, inspect, roll back, verify no
  residue.
- Panel/layout changes are manual documentation only in V0.1.

Packs that need shell commands, absolute paths, or unsupported resources will be rejected;
do not propose workarounds.

## Adding a future desktop adapter

Not in V0.1. If you are proposing one:

1. Open an issue and write an ADR (see [DECISIONS.md](DECISIONS.md) "Adding an ADR").
2. Get agreement that the adapter contract is sufficient. If it is not, revise
   ARCHITECTURE first.
3. Implement `DesktopAdapter` with its own probe, target map, and verification strategy.
4. Add operation executors only if the desktop needs kinds KDE does not have; each must
   implement validate/apply/verify/reverse.
5. Do not change the pack schema, planner, transaction engine, snapshot format, or database
   for one adapter.
6. Add adapter unit tests with fakes and a real-VM integration suite.
7. Update README, PRD supported platforms, MVP scope, SECURITY trust boundaries, TESTING,
   PHASES, and AGENTS.md.

## Testing expectations

- Every mutation path needs tests at the transaction level, including a failure at that
  operation and a verified rollback.
- Every new `ErrorCode` needs a test that produces it.
- Parser/validator changes need a fixture per new rejection rule and a fuzz seed.
- Filesystem code needs temp-directory tests; never use the real `~/.config`.
- Adapter changes need fake-bus tests and, where behavior is version-dependent, VM tests.
- If you cannot test something, say so in the PR and add it to the phase exit criteria;
  do not mark it as verified.

See [TESTING.md](TESTING.md) for the full strategy and required failure matrix.

## Proposing and submitting changes

1. **Issue first** for anything non-trivial: behavior change, new module, new dependency,
   schema change, UI flow change. Describe the problem, not just the solution.
2. **ADR** for architectural changes (see list at the end of [DECISIONS.md](DECISIONS.md)).
3. **Small PRs** with one concern. Refactors separate from behavior changes.
4. **Reference** the relevant `FR-`, `AC-`, `ADR-`, or threat ID in the PR description.
5. **Report** tests run and their results. If a check failed, say so.
6. **Do not** weaken assertions, skip tests, add lint suppressions, or silence errors to get
   green CI. Fix the cause or explain why the behavior itself must change.
7. **Dependencies**: justify any new crate or npm package (purpose, std alternative,
   maintenance, license, security surface).

Commit messages: imperative subject under ~72 characters; body explains *why* and any
tradeoff. Keep unrelated changes out of the commit.

## Documentation updates

| Change | Update |
| --- | --- |
| New capability, operation, or target | ARCHITECTURE, PACK_SPEC, MVP, SECURITY, DECISIONS (ADR) |
| Transaction/rollback behavior | TRANSACTIONS, TESTING, MVP acceptance criteria |
| Schema or pack field | PACK_SPEC, DATABASE, fixtures, this file if authoring changes |
| UI flow | README screenshots/description once they exist, TESTING |
| Supported Plasma versions/distros | README, PRD, PHASES Phase 11 notes |
| Security control | SECURITY, TESTING, ARCHITECTURE if boundaries move |
| New module | ARCHITECTURE module tree and dependency table |

Documentation is part of the change, not a follow-up.

## Definition of done for a contribution

- [ ] Behavior matches MVP/PRD; no unapproved scope.
- [ ] Architecture boundaries respected (`tests/architecture.rs` passes).
- [ ] Typed errors; no new panics on untrusted input.
- [ ] Tests added, including failure paths; all checks pass.
- [ ] No security control weakened; threat model updated if a new surface exists.
- [ ] Docs updated; ADR added if architectural.
- [ ] UI states cover empty, loading, unsupported, failure, and recovery where relevant.
- [ ] Accessibility checked for UI changes.
- [ ] No secrets, personal paths, or generated junk committed.

## Reporting security issues

Do not open a public issue. Use the repository's private security advisory feature as
described in [SECURITY.md](SECURITY.md) §17.

## Conduct

Be direct, technical, and respectful. Critique designs and code, not people. Assume good
faith, and prefer demonstrating a problem with a test or a reproduction over asserting it.
