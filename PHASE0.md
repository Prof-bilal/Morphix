# Morphix — Phase 0 Execution Plan

- **Status:** Phase 0 in progress. Stages A–E, G, and Stage F except its CI workflow
  (P0-10, P0-09) have landed. P0-08, the CI workflow, is pending and
  [TASKS.md](TASKS.md) carries the per-task state.
- **Scope:** P0-01 … P0-10 only. No Phase 1 work belongs here.
- **Relationship to other documents:** [TASKS.md](TASKS.md) stays the checklist of record
  and [PHASES.md](PHASES.md) the phase definition. This file only adds sequencing,
  verification commands, and the environment facts the first session depends on. Delete it
  when every Phase 0 box is checked.

## 1. Where the repository stands

Verified on 2026-10-19, from a clean tree:

| Check | Result |
| --- | --- |
| Git state | started from `abba431` on `main`, clean, in sync with `origin/main` |
| Tracked content at plan time | 20 files: 17 Markdown documents, `LICENSE-MIT`, `LICENSE-APACHE`, `.gitignore` |
| Code at plan time | none — no `*.rs`, `*.toml`, `*.ts`, `*.tsx`, `*.json`, `*.yml`, `*.py`, `*.html`, `*.css`, `*.lock` |
| Landed since | Stages A–E: baseline files, the workspace with the `morphix-core` module skeleton, the error taxonomy (73 codes with registry drift tests), the architecture boundary test, the Vite/React shell, and the `tracing` logging bootstrap. Stage G: the Tauri 2 shell with `get_app_info` and a window |
| Contracts to satisfy | 32 acceptance criteria, 15 capabilities, 8 UI routes, 41 error codes in [ARCHITECTURE.md](ARCHITECTURE.md) §12.1 plus the validation codes in [PACK_SPEC.md](PACK_SPEC.md) §8, 22 ADRs |

### Toolchain measured on this machine

| Need | Status |
| --- | --- |
| `rustc` / `cargo` 1.99.1 | present |
| `node` 24.22.0, `npm` 11.19.1 | present |
| `python3` 3.14.8 | present (documentation checks) |
| `gtk+-3.0` 3.24.52, `librsvg-2.0` 2.63.3, `openssl` 3.6.5 | present |
| `webkit2gtk-4.1` 2.54.1 | present (B-1 resolved) |
| Desktop | Plasma 6.8.0 on Wayland — the validated combination |
| Disk | 209 GB free |

## 2. Resolved blocker

**B-1 — `webkit2gtk-4.1` was not installed.** It is now installed (2.54.1), so `src-tauri`
compiles and the second half of P0-02 is complete. `cargo build -p morphix-app` succeeds;
the remaining launch check waits on the frontend scaffold (P0-06), which replaces the
temporary `src-tauri/ui/index.html` placeholder.

## 3. Decisions applied unless you object

| # | Decision | Default |
| --- | --- | --- |
| D-1 | Rust pin | exact `1.99.1` in `rust-toolchain.toml`, with `rustfmt` and `clippy` components |
| D-2 | Tauri | 2.x stable; no `fs`, `shell`, or `http` plugin (AC-25); capabilities file limited to the Phase 0 command |
| D-3 | Frontend | npm, Vite, React, TypeScript, React Router, TanStack Query, Vitest + Testing Library, plain CSS tokens |
| D-4 | CI | GitHub Actions on `ubuntu-24.04`; Rust, Architecture, Frontend, Docs jobs; no KDE VM job in Phase 0 |
| D-5 | License metadata | `MIT OR Apache-2.0` in both manifests |
| D-6 | Commit style | one stage per commit, imperative subject naming tasks: `Add workspace and core module skeleton (P0-02, P0-03)` |

These are cheap to change now and expensive after ten files depend on them. If any is
wrong, say so before Stage A.

## 4. Stages

Each stage is one reviewable change with its own tests. A stage is not "done" until its
green commands have actually been run and their exit codes recorded.

### Stage A — Baseline hygiene (P0-01)

**Produces**

- `.gitignore` gains `/target/`, `/node_modules/`, `/dist/`, `*.tsbuildinfo` on top of the
  existing `.contextgit/` rule. Fixture directories under `tests/` are deliberately not
  ignored, so hostile-pack fixtures stay committable.
- `.editorconfig`, `rust-toolchain.toml`, `rustfmt.toml`.
- The two license files already exist; nothing to add.

**Green when**

```bash
rustup show active-toolchain        # 1.99.1 pinned by the file, not by "stable"
cargo fmt --version                 # resolves under the pinned toolchain
git status --short                  # only the four intended files
```

### Stage B — Workspace and module skeleton (P0-02 core half, P0-03)

**Produces**

- Root `Cargo.toml`: workspace with `members = ["crates/morphix-core"]`, workspace
  package/dependency tables, and `unsafe_code = "forbid"`; `src-tauri` joins the members
  list in Stage G.
- `crates/morphix-core/Cargo.toml`: `serde`, `serde_json`, `thiserror`, `semver`;
  dev-dependencies added as their first test needs them.
- `src/lib.rs`: the `#![forbid(unsafe_code)]` and `#![deny(clippy::unwrap_used, …)]`
  block required by [AGENTS.md](AGENTS.md) §11 and [CONTRIBUTING.md](CONTRIBUTING.md), and
  the thirteen top-level modules from ARCHITECTURE §4 (`core`, `desktop`, `errors`, `fs`,
  `logging`, `packs`, `permissions`, `planning`, `platform`, `snapshots`, `storage`,
  `system`, `transactions`), each with its ownership doc comment and no logic. The deny
  list lives in `lib.rs`, not in workspace lints, so test targets can still `unwrap` a
  broken assertion.
- Nested files (`desktop/kde/*.rs`, `storage/repo/*.rs`, …) are created when their first
  task lands, not now. Sixty empty files would be churn and would make the architecture
  test assert on modules that have no edges yet.

**Green when**

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Stage C — Error taxonomy (P0-04)

**Produces**

- `errors/code.rs`: every code from ARCHITECTURE §12.1 and PACK_SPEC §8 in one table macro,
  with `as_str` and a `FromStr` implementation that returns a typed `UnknownErrorCode`
  instead of panicking.
- `errors/mod.rs`: `MorphixError` (code, message, optional detail, ordered context),
  `Display` as `<CODE>: <message>`.
- `errors/report.rs`: `RollbackOutcome` whose `Succeeded` variant cannot be built without
  verified restoration, `OperationOutcome`, `FailureReport`, `RecoveryReport`.
- Tests: round-trip through JSON for every code; unknown code string rejected; display
  format; a drift test that compares the table against the code list in the documents.

**Green when**

```bash
cargo test -p morphix-core errors
```

### Stage D — Architecture enforcement (P0-05)

**Produces**

- `crates/morphix-core/tests/architecture.rs`: the allowed/forbidden matrix from
  ARCHITECTURE §5 as data, a scanner over `src/**/*.rs` for `use crate::<module>`, and a
  meta-test that runs the same scanner over an inline fixture graph and asserts the
  forbidden edge is reported. The meta-test is what proves the check is not vacuous.

**Green when**

```bash
cargo test -p morphix-core --test architecture
```

plus a manual demonstration in the PR: inject one forbidden import, show the test failing,
revert it.

### Stage E — UI scaffold (P0-06)

**Produces**

- `package.json`, `package-lock.json`, `index.html`, `vite.config.ts`, `tsconfig.json`.
- `src/main.tsx`, `src/App.tsx`, the eight routes from [UI_SPEC.md](UI_SPEC.md) §3 as
  placeholders, `src/styles/tokens.css` with exactly the tokens in UI_SPEC §13.
- `src/api/client.ts` as a stub with no `invoke` calls yet (P0-07 fills it in).
- One smoke test that renders the shell and asserts the navigation entries.

**Green when**

```bash
npm ci
npm run typecheck
npm run test -- --run
npm run build
```

### Stage F — CI, docs checks, logging (P0-08, P0-09, P0-10)

**Produces**

- `.github/workflows/ci.yml` with four jobs: Rust (fmt, clippy, test), Architecture,
  Frontend (typecheck, test, build), Docs.
- `tests/check_docs.py`: capability IDs agree across MVP §4, ARCHITECTURE §6, PACK_SPEC §6;
  ADR sequence and index complete; AC sequence; every internal link resolves; every
  document appears in the README index; every command CONTRIBUTING marks available exists
  and every command it marks planned does not. `--self-test` runs the same checks on a
  broken copy and asserts both a broken link and a capability mismatch are reported.
- ADR template in [DECISIONS.md](DECISIONS.md) (next free number is ADR-023).
- `logging.rs`: `tracing` subscriber writing to the XDG state directory with mode `0600`,
  size-based rotation, and a redaction filter; a test proving a config value and a file
  body handed to the logger never appear in the output. Phase 0 uses a plain
  `OpenOptions::mode(0o600)` + rename write, because `fs::atomic` is a Phase 4 deliverable.

**Green when** every command in §5 exits 0, and the docs checker is shown to fail on a
deliberately broken link in a scratch copy — not in the repository.

### Stage G — Tauri shell (P0-02 second half, P0-07) — landed early

**Produces**

- `src-tauri/Cargo.toml`, `build.rs`, `tauri.conf.json`, `capabilities/default.json` with
  only the Phase 0 command.
- `src-tauri/src/{main,lib,state,error,events}.rs` and `commands/app.rs` answering
  `get_app_info` with app version, data directory, and state directory.
- `src/api/client.ts` becomes the only module that calls `invoke`.

**Green when**

```bash
cargo check -p morphix-app        # passes
cargo test  -p morphix-app        # passes (3 command tests)
npm run tauri dev                 # wired; window launch observed during Phase 0 exit
```

## 5. Verification contract

Run from the repository root, in this order, and report exit codes rather than filtered
success lines:

| Check | Command |
| --- | --- |
| Rust format | `cargo fmt --all --check` |
| Rust lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust tests | `cargo test --workspace` |
| Architecture | `cargo test -p morphix-core --test architecture` |
| Frontend install | `npm ci` |
| Frontend types | `npm run typecheck` |
| Frontend tests | `npm run test -- --run` |
| Frontend build | `npm run build` |
| Documents | `python3 tests/check_docs.py` |

Two environment notes for this machine, both observed already:

- `git` over HTTPS needs `env -u LD_LIBRARY_PATH -u LD_PRELOAD`, because the packaged
  shell's libcurl shadows the system one (`curl_global_trace` undefined symbol).
- The documentation checker runs as
  `env -u PYTHONHOME -u PYTHONPATH python3 tests/check_docs.py`, because the packaged
  shell exports a `PYTHONHOME` that points at its own interpreter and shadows the system
  stdlib. `--self-test` proves the checker fails on a broken copy.
- Every test from Stage C onward sets `HOME`, `MORPHIX_DATA_DIR`, and `MORPHIX_STATE_DIR`
  to temporary directories. A test that writes to the real home is a defect, not a
  convenience (TESTING §1.2).

## 6. Definition of done for Phase 0

Criterion, and where it is proven:

| Criterion | Proof |
| --- | --- |
| All nine gates in §5 pass locally and in CI | CI run linked in the PR |
| `tests/architecture.rs` fails on a forbidden import | meta-test plus the demonstrated injection |
| App launches and shows the placeholder UI | Stage G plus the P0-06 Vite build |
| Every document linked from README; no doc claims an unimplemented feature | `tests/check_docs.py` |
| No domain logic exists | `cargo test` has no planner, pack, snapshot, or transaction tests |

## 7. Risks

| Risk | Mitigation |
| --- | --- |
| Architecture test becomes a brittle source parser | Matrix is data, not code; only `use crate::` edges are inspected; the meta-test exercises the scanner on a fixture |
| Doc-drift test breaks on innocuous document edits | Drift test asserts set equality with one clear message; if it proves noisy, downgrade to a count assertion and record why |
| Error table churn across 41+ codes | One macro table; adding a code is one line plus its doc row |
| Toolchain drift between machines and CI | Exact pin in `rust-toolchain.toml`; `Cargo.lock` committed |
| Scope creep into Phase 1 while "the workspace already exists" | Phase 0 PRs add no detection, pack, or planning code; the test count stays at the Phase 0 suites |
| Tauri dependency churn | Deferred to Stage G, and only after the system dependency exists |

## 8. First session, in order

1. Branch: `git switch -c p0-foundation`.
2. Stage A, commit.
3. Stage B, run the three Rust gates, commit.
4. Stage C — the largest single piece; the code table and its round-trip tests in one
   commit.
5. Stage D, demonstrate the failing injection, commit.
6. Stop and report: gates green, what the diff contains, what Stage E needs next.

Stage E and F are a second session; Stage G waits on B-1.

## 9. Explicit non-goals for Phase 0

- No pack parsing, no detection, no planning, no SQLite, no DBus, no KDE paths.
- No Tauri plugin, network client, telemetry, or auto-update.
- No UI beyond placeholder routes, design tokens, and the shell.
- No capability labelled as working before [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) says
  it is.
