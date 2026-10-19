<!-- contextgit:begin -->
## Parallel runs (managed by ContextGit)

Each run works in its own git worktree. Stay inside the files you own;
if you must touch a file owned by another run, say so before editing.

<!-- contextgit:end -->

# AGENTS.md — Instructions for AI Coding Agents

This repository is **Morphix**, a safe, reversible desktop customization platform for
**KDE Plasma 6**. You are working on a security-sensitive system that writes user
configuration. Read this file completely before making changes.

## 1. Mandatory reading order

Before writing any code:

1. `README.md`
2. `PRD.md`
3. `MVP.md`
4. `ARCHITECTURE.md`
5. `SECURITY.md`
6. The document for the area you are changing: `PACK_SPEC.md`, `TRANSACTIONS.md`,
   `DATABASE.md`, `TESTING.md`
7. `DECISIONS.md` (ADRs are binding)
8. `TASKS.md` (the task you are implementing, and its phase in `PHASES.md`)

If these documents conflict, stop and ask for a decision. Do not pick the interpretation
that is easiest to implement.

## 2. Current state

- Take the current state from `README.md` and `TASKS.md`, not from memory. As of Phase 0
  in progress, the repository contains the specifications, the Cargo workspace, the
  `morphix-core` module skeleton, and the error taxonomy — nothing else.
- Do not pretend commands, modules, tests, or binaries exist that the repository does not
  contain.
- Do not mark tasks complete in `TASKS.md` unless the implementation, tests, and docs are
  actually present and verified.
- Do not add "future" scaffolding that the current phase does not require.

## 3. Hard scope rules

- **KDE Plasma 6 only** (`>=6.0, <7.0`). Never add GNOME, Hyprland, XFCE, Cinnamon, or
  generic desktop support without a new ADR that supersedes ADR-004.
- **Panel, widget and desktop-behaviour changes are in V0.1**, but only through the
  ADR-018 operation kinds with read-back verification and reverse operations. Never
  restart plasmashell, KWin, or the session, and never invoke `systemctl` (ADR-020).
  Plasma scripting is a trusted-generator boundary only, never pack text (ADR-021).
  Panel opacity is refused; it is not a supported property (ADR-018).
- **No network, accounts, telemetry, registry, or auto-update** (ADR-008). No HTTP client.
- **No root, daemon, setuid, polkit, or privileged helper** (ADR-016). Refuse to mutate
  when `geteuid() == 0`.
- **No pack-provided scripts, shell, interpreters, binaries, or hooks** (ADR-006).
- **No pack-declared filesystem paths** (ADR-010). Targets come only from the fixed map.
- **No direct Look-and-Feel package application** (ADR-012, superseded by ADR-019).
  Global themes are decomposed into primitive, reversible operations instead.
- **No arbitrary KDE DBus methods or undocumented APIs invented from memory.**
- **Do not silently expand scope because a feature seems useful.** File it as a
  post-MVP possibility instead.

## 4. File ownership and parallel runs (ContextGit)

- Each run works in its own worktree. Stay inside the files you own.
- If you must edit a file owned by another run, state that before editing it.
- Do not revert or reformat unrelated work.
- Do not create commits, push, or change git configuration unless the task explicitly asks.

## 5. Where code goes

```
crates/morphix-core/    all domain logic (Rust, no Tauri/GUI dependencies)
  errors/               MorphixError, ErrorCode, reports
  core/                 IDs, versions, capabilities, resource types
  platform/             XDG paths, os-release, package DB, session, version probes
  fs/                   FileSystem trait, SafePath, atomic writes, tree hashing, lock
  desktop/              DesktopAdapter trait (+ kde/ implementation)
  packs/                manifest, parser, validator, integrity, store, catalog
  permissions/          allowlists, capability resolution, consent
  planning/             plan model, planner, expansion, diffs, collisions
  snapshots/            capture, object store, manifest, restore
  transactions/         engine, journal, executor, recovery
  storage/              SQLite, migrations, repositories
  system/               detection orchestration and support verdict
  logging.rs            tracing setup and redaction
src-tauri/              thin Tauri 2 shell: commands, DTOs, events, state
src/                    React + TypeScript UI (presentation only)
packs/                  first-party declarative demo packs
tests/                  cross-cutting fixtures and integration tests
```

- Put code in the module that owns the concern. Do not create `utils.rs`, `helpers.rs`, or
  `common.rs`.
- `desktop::kde` is the only place that may know KDE paths, config file names, or KDE DBus
  interfaces.
- `storage` is the only place that may contain SQL.
- `transactions` is the only place that may mutate user state.
- `planning` must remain read-only.
- `src-tauri` commands must be thin: validate DTO shape, call one service method, map the
  result. No filesystem, DBus, SQL, or business logic in commands.
- The frontend must never construct KDE paths, write files, call DBus, or call `invoke`
  outside `src/api/client.ts`.

## 6. Dependency direction rules

Follow `ARCHITECTURE.md` §5 exactly. In particular:

- `errors` is a leaf (std/serde/thiserror only).
- `core` does not depend on fs, desktop, storage, or planning.
- `fs` does not depend on packs, desktop, storage, or planning.
- `desktop` does not depend on packs, planning, transactions, or storage.
- `packs` does not depend on planning, transactions, or desktop.
- `planning` does not depend on transactions or storage.
- `transactions` may depend on planning, snapshots, desktop, fs, storage.
- `storage` does not depend on desktop, planning, or transactions.
- No core module depends on Tauri, WRY, WebKitGTK, or any GUI crate.

If an import feels necessary but violates the table, the design is wrong; propose an
architecture change with an ADR instead of editing the test.

## 7. Security rules (non-negotiable)

1. Packs are untrusted input. Only `packs::parser`/`validator` may see raw manifest text;
   everything downstream consumes typed models.
2. Never execute a child process for pack content. The only permitted processes are the two
   fixed-argument version probes in `platform::probe`; no shell, no `sudo`, no `pkexec`.
3. Never write outside the fixed target map. Do not add a path parameter to a command or an
   operation.
4. Every mutation needs a verified snapshot and a durable journal entry first.
5. Never report success without verification. If rollback could not be verified, report
   `rollback_failed`/`recovery_required`, not "restored".
6. Refuse symlinked write targets and symlinks in pack content. Do not "follow just this
   one" for convenience.
7. Do not weaken or bypass validation to make a test pass.
8. Do not add a dependency that brings network, shell, or filesystem-server capability into
   the core.
9. Do not log file contents, asset bytes, or secrets.
10. Keep the Tauri capability set minimal; do not expose fs/shell/http plugins to the
    webview.

## 8. KDE uncertainty protocol

KDE behavior varies by version and is often undocumented. Before implementing anything that
touches KDE:

1. Write down the assumption and the range of versions it applies to.
2. Prefer a documented DBus interface or a KConfig key with a known format.
3. If no stable interface exists, document the fallback and the risk in the relevant doc.
4. Add a probe so unsupported systems get a clear refusal or `Degraded` capability, not a
   silent failure.
5. Add a test that pins the generated command/script/config shape.
6. If the behavior cannot be verified in the current environment, say so explicitly in the
   PR and mark the task incomplete; do not fabricate API names or claim it works.

Never invent a DBus method, config key, file path, or KDE package layout. If you are not
certain, check the existing docs in this repository, then ask, then prototype.

## 9. Mutation and transaction rules

- Read `TRANSACTIONS.md` before changing anything that writes.
- Every operation kind must define: validation, apply, verify, and reverse.
- Every write must be reversible with the recorded prior state.
- Apply order is deterministic; rollback is reverse order.
- Do not add an operation kind that cannot verify its effect.
- Do not reverse by rewriting a whole file when a conditional per-key reversal is required;
  that clobbers newer user changes.
- Do not delete a path Morphix did not create (except restoring a snapshot that recorded it
  as present).
- Do not add automatic retention/pruning of snapshots.
- Do not add an unchecked "force" flag. Conflict overrides must be explicit, recorded in
  history, and require user confirmation.

## 10. Error handling and logging

- Public core APIs return `Result<T, MorphixError>` with a stable `ErrorCode`.
- Add a new `ErrorCode` only when no existing code fits; codes are additive and never
  repurposed.
- Errors must state what failed, the target in user terms, whether anything changed, and
  whether rollback succeeded.
- No `unwrap()`/`expect()` on I/O or untrusted input. Tests may use `expect` with context.
- No silent `let _ = ...` on a fallible operation that affects state.
- Log structured fields (`transaction_id`, `plan_id`, `pack_id`, target, error code).
  Never log file contents or asset bytes.
- Do not swallow errors to make a flow appear successful.

## 11. Rust conventions

- `#![forbid(unsafe_code)]` in `morphix-core`. Any exception requires an ADR and a safety
  argument.
- Use newtypes for IDs and paths (`PackId`, `SafePath`) rather than bare `String`/`PathBuf`
  across boundaries.
- Model closed sets as enums with explicit serde renames; never accept arbitrary strings
  where an enum exists.
- Keep modules focused; target under ~600 non-test lines. Split by responsibility.
- Prefer explicit types at module boundaries; derive `Debug`, `Clone`, `PartialEq` where
  useful, and `Serialize`/`Deserialize` only for types that cross a boundary.
- Use `thiserror` for domain errors; do not leak `rusqlite`, `zbus`, or `std::io` errors
  through public APIs without mapping.
- Use `spawn_blocking` in commands for core work; never block the async runtime.
- Comments explain *why*, not *what*. Mark non-obvious KDE behavior with a doc comment and
  a link to the assumption/ADR.

## 12. TypeScript / React conventions

- `strict` TypeScript; no `any` in application code (test doubles may use `unknown` with
  narrowing).
- IPC types come from Rust via `ts-rs`; do not hand-edit generated files.
- Only `src/api/client.ts` calls `invoke`.
- No `dangerouslySetInnerHTML`; render untrusted text as text.
- No remote fonts, scripts, or images; the app is offline and CSP-restricted.
- Failure and recovery states must be visually distinct; never present a failed rollback as
  success.
- Keep components small; extract plan rendering, capability lists, and error banners.
- All interactive elements must be keyboard reachable with visible focus.
- Add React tests for behavior, not snapshots of markup alone.

## 13. Database conventions

- Follow `DATABASE.md`. All SQL lives in `storage`, parameterized.
- Never interpolate pack text into SQL, including table/column names.
- Journal writes are durable and precede the corresponding filesystem action.
- History is append-only; do not add update/delete paths.
- Migrations are sequential and immutable once released. Never edit an existing migration.
- New tables/columns require a schema migration, repository tests, and a docs update.

## 14. Testing requirements

For every change:

- Unit tests for new pure logic.
- Integration tests for module interactions.
- A failure test for every new error path that can affect state.
- A rollback test for every new operation kind (apply, verify, reverse, conflict).
- Temp-directory isolation; never touch the real `~/.config` or session bus.
- Parser/validator changes: a fixture per new rejection rule.
- Path/security changes: a traversal/symlink/limit test.
- UI changes: component tests for success and failure states, plus keyboard access.
- Crash/recovery changes: a `CrashSimulator` boundary test.

Run the checks that exist. If a command does not exist yet, say so instead of claiming it
passed. Never mark a task complete with failing or skipped tests.

## 15. Documentation rules

Update documentation in the same change as the code:

| Change | Documents to update |
| --- | --- |
| New capability/operation/target | `ARCHITECTURE.md`, `PACK_SPEC.md`, `MVP.md`, `SECURITY.md`, `DECISIONS.md` |
| Transaction/rollback/snapshot behavior | `TRANSACTIONS.md`, `TESTING.md`, `MVP.md` |
| Pack schema/validation | `PACK_SPEC.md`, `DATABASE.md` if persisted, fixtures |
| IPC command/DTO | `ARCHITECTURE.md` §13, generated TS types, command tests |
| UI flow | `CONTRIBUTING.md` if contributor-facing, `TESTING.md`, README when shipped |
| Supported platform/version | `README.md`, `PRD.md`, `PHASES.md`, release notes |
| Security control | `SECURITY.md`, `TESTING.md`, relevant ADR |

Architectural changes require an ADR first. Do not rewrite history in existing ADRs; add a
superseding ADR.

## 16. Definition of done for agent work

A task is complete only when all of the following are true:

- [ ] The behavior matches the current phase and `MVP.md`; no unapproved scope.
- [ ] Architecture boundaries pass (`tests/architecture.rs` once it exists).
- [ ] All new code has typed errors and no new panics on untrusted input.
- [ ] Tests cover success and failure; rollback/recovery paths are verified where relevant.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, frontend typecheck/tests/build
      pass (once those commands exist).
- [ ] No security control was weakened; no test was skipped or assertion relaxed.
- [ ] Documentation and the task checkbox are updated honestly.
- [ ] Any unresolved assumption is written down, with a proposed prototype or follow-up.

If you cannot meet a criterion, stop and report exactly what is missing. Do not claim
completion.

## 17. When blocked

- Do not guess KDE APIs, file formats, or package layouts.
- Do not invent a requirement that is not in the docs.
- Do not implement a workaround that bypasses the transaction engine.
- Write down the question, the options, and the risk, and ask the maintainer. A short
  blocker report is better than plausible-looking code that violates the safety model.
