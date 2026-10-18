# Morphix — Testing Strategy

- **Status:** Design baseline. No tests exist yet; this defines what must be built.
- **Related:** [MVP.md](MVP.md) §7 (acceptance criteria), [TRANSACTIONS.md](TRANSACTIONS.md),
  [SECURITY.md](SECURITY.md), [PHASES.md](PHASES.md).

Morphix's core value is trustworthiness, so testing is failure-first: the interesting cases
are the ones where an operation fails, a file changes underneath us, a snapshot is
unreadable, or the process dies mid-write. Success-path coverage alone is not acceptance.

---

## 1. Principles

1. **Core is headless-testable.** No test of `morphix-core` requires Tauri, a display
   server, or a running Plasma session, except the explicitly gated KDE integration suite.
2. **Never touch the real user configuration.** All tests set `MORPHIX_DATA_DIR`,
   `MORPHIX_STATE_DIR`, and `HOME` to temporary directories. A test that writes to the
   developer's `~/.config` is a bug.
3. **Deterministic.** `FakeClock`, fixed UUID seeds where ordering matters, sorted
   iteration, and canonical serialization.
4. **Fault injection is a first-class tool**, not an afterthought.
5. **Every acceptance criterion maps to at least one automated test.** The traceability
   matrix is CI-checked (TASKS P11-01).
6. **Rollback is tested at every boundary**, including crashes inside recovery.
7. **No test asserts on unstable text.** Error assertions use `ErrorCode`, not prose.
8. **VM tests are real.** Adapter integration runs on disposable Plasma 6 VMs, never on a
   developer's live session.

## 2. Test layers

| Layer | Location | Scope | Speed |
| --- | --- | --- | --- |
| Unit | `#[cfg(test)]` beside modules | Pure functions, parsers, state machines | ms |
| Module integration | `crates/morphix-core/tests/` | Planner, snapshots, transactions with fakes | ms–s |
| Filesystem integration | same, with `tempfile` | Real atomic writes, symlinks, permissions | s |
| Security fixtures | `tests/fixtures/packs/` | Malicious/hostile packs | ms |
| Adapter contract | `desktop::kde` tests with `FakeBus` | Command construction, parsing, error mapping | ms |
| Adapter integration | `tests/kde_integration.rs` (`#[ignore]`, env-gated) | Real Plasma 6 VM: files, DBus, read-back | minutes |
| IPC contract | `src-tauri` tests | Command DTOs, error mapping, events | ms |
| Frontend unit/component | `src/**/*.test.tsx` | Rendering, consent gating, error states | s |
| End-to-end manual/scripted | `tests/e2e/` + VM script | Install → apply → rollback → restore → crash | minutes |

## 3. Required harnesses

| Harness | Purpose |
| --- | --- |
| `FakeFileSystem` | In-memory tree; inject errors at an exact call index or predicate; record all calls |
| `RecordingFileSystem` | Wraps the real FS and asserts zero write calls during planning |
| `FakeAdapter` | Configurable probe/capabilities; records validate/apply/verify/reverse calls; can fail at a chosen operation |
| `FakeBus` | Records DBus method calls and returns scripted replies/errors |
| `FakeClock` | Fixed and advancing time for expiry/ordering |
| `CrashSimulator` | Runs the engine with a hook that aborts at a named boundary (`after_snapshot`, `before_op_apply`, `after_op_apply`, `after_verify`, `during_rollback`, ...) |
| `TempHome` | Creates an isolated HOME/XDG tree and restores env on drop |
| `PackBuilder` | Builds valid/invalid packs from templates, generates checksums |
| `TreeSnapshot` | Captures a directory tree (paths + hashes + modes) for before/after comparison |

## 4. Test suites

### 4.1 Parsers and manifest validation

- `platform::os_release`: Arch, Fedora, Ubuntu, openSUSE, Debian, missing file, empty file,
  malformed lines, quoted values, `ID_LIKE`, very long line.
- `core::versions`: `6.0`, `6.0.0`, `6.3.5`, `6.10.1`, `6.4.0-rc1`, `5.27.11`, garbage,
  overflow, leading zeros. Constraint parsing: `>=6.0`, `>=6.0, <7.0`, `^6.3`, `*`,
  `>=5.0` (must be rejected as a v1 pack), empty, inverted.
- `packs::parser`: valid minimal pack, full pack, unknown field, missing field, wrong type,
  duplicate key, duplicate table, deeply nested TOML, 256 KiB boundary, invalid UTF-8,
  BOM, CRLF, Unicode names, NUL byte.
- `packs::validator`: one fixture per error code in [PACK_SPEC.md](PACK_SPEC.md) §8,
  including capability declared/unused/undeclared, appearance referencing a missing asset,
  settings key not allowlisted, effect/shortcut not allowlisted, duplicate target,
  dependency cycle, dependency missing, homepage URL, unreferenced file, executable file,
  symlink, hardlink, FIFO, oversize file, too many files, depth limit.
- `packs::integrity`: valid checksums, bad hash, missing entry, extra entry, duplicate
  path, absolute path, traversal path, uppercase hex, wrong separator.
- Property tests (proptest): parser never panics for arbitrary bytes up to 256 KiB;
  path validator never accepts a path that escapes the pack root.

### 4.2 Planner

- Determinism: two runs on identical state produce byte-identical canonical plans and the
  same `PlanHash`; ordering is stable.
- Purity: `RecordingFileSystem` + `FakeAdapter` observe zero mutations.
- Expansion: each `[appearance]` field produces exactly the expected operations and
  config keys; `cursor_size` without `cursor_theme` is rejected.
- Capability grants: requested set equals granted set; undeclared use rejected; unused
  request rejected.
- Compatibility: exact version vs range, malformed range, range admitting Plasma 5.
- Collisions: target owned by another pack, pre-existing unowned target, duplicate target
  within one plan, asset key reused across kinds.
- Drift/preconditions: hash changes between plan and apply ⇒ `E_PLAN_STALE`, zero writes.
- Expiry: expired plan rejected before the lock.
- Wallpaper gate: probe unavailable ⇒ no `SetWallpaper`, `manual_step` warning, plan hash
  stable.
- Refusal: every unsupported resource fixture yields the expected code and no plan.

### 4.3 Snapshot engine

- Capture of a config file, a config key, an absent target, a directory tree, and a
  wallpaper descriptor.
- Object dedup across two snapshots with identical content.
- Manifest self-hash; tamper with one byte ⇒ `E_SNAPSHOT_CORRUPT`.
- Missing/truncated object ⇒ apply refused before mutation.
- `snapshot_complete` is never written unless every object verified (spy on journal).
- Disk full during object write ⇒ `failed`, no mutation, no partial snapshot marked
  complete.
- Permission denied on a snapshot target ⇒ `failed` with `E_IO_PERMISSION`.
- Restore plan: exact file restore, directory tree replacement, absent path removal,
  drift warnings, and refusal when an object hash fails.
- Snapshot path traversal via a crafted manifest (defense in depth) is rejected.

### 4.4 Security

Fixtures in `tests/fixtures/packs/` (committed, no real malware):

| Fixture | Expected |
| --- | --- |
| `traversal-asset` (`../../.ssh/authorized_keys`) | `E_PATH_TRAVERSAL` |
| `absolute-asset` (`/etc/passwd`) | `E_INVALID_PATH` |
| `symlink-escape` (asset symlink to `/etc`) | `E_SYMLINK_REJECTED` |
| `hardlink-asset` | `E_SYMLINK_REJECTED` |
| `executable-asset` | `E_FILE_TYPE_REJECTED` |
| `shell-script` (`install.sh`) | `E_UNREFERENCED_FILE` or `E_FILE_TYPE_REJECTED` |
| `unknown-capability` | `E_CAPABILITY_UNKNOWN` |
| `undeclared-capability` | `E_CAPABILITY_UNDECLARED` |
| `unused-capability` | `E_CAPABILITY_UNUSED` |
| `disallowed-key` (`kdeglobals [KDE] SingleClick=false`) | `E_CONFIG_KEY_NOT_ALLOWED` |
| `duplicate-target` | `E_DUPLICATE_TARGET` |
| `oversize-wallpaper` | `E_LIMIT_EXCEEDED` |
| `too-many-files` | `E_LIMIT_EXCEEDED` |
| `deep-path` | `E_LIMIT_EXCEEDED` |
| `bad-checksum` | `E_CHECKSUM_MISMATCH` |
| `incomplete-checksums` | `E_CHECKSUM_INCOMPLETE` |
| `plasma5-range` | `E_COMPATIBILITY_INVALID` |
| `unknown-field` | `E_UNKNOWN_FIELD` |
| `schema-v2` | `E_SCHEMA_VERSION_UNSUPPORTED` |
| `description-xss` (`<img onerror=...>`) | accepted as text; UI renders escaped; test asserts no HTML injection |
| `homepage-javascript` | `E_VALUE_INVALID` |

Additional security tests:

- A test-only pack reader asserts that `Data` files cannot be opened outside the pack root.
- Static check: no `std::process::Command` outside `platform::probe`; no `sudo`/`pkexec`
  strings; no `sh -c`.
- Static check: no `dangerouslySetInnerHTML` in `src/`.
- Tauri capability manifest exposes only the expected commands; CSP has no remote sources.
- Path safety property test: generated paths either reject or remain under an allowed root.
- TOCTOU test: swap an ancestor directory for a symlink between validation and write where
  the harness can race deterministically; the write must fail rather than escape.
- Running with `geteuid() == 0` (test double) refuses mutation.
- Config file that is a symlink ⇒ `E_SYMLINK_TARGET`.
- Oversized config file ⇒ `E_CONFIG_TOO_LARGE`.
- Corrupt DB ⇒ mutations blocked, no panic, diagnostics available.

### 4.5 Transaction and rollback (priority suite)

For every bundled pack and a synthetic pack covering all five operation kinds:

1. **Happy path:** transaction committed, all ops verified, installation row updated,
   history event written.
2. **Failure at op N of M, for all N:** apply fails, rollback runs, every pre-N op is
   reverted in reverse order, restoration verified, transaction `rolled_back`, no
   `committed` installation row, history records failure and rollback success.
3. **Verification failure at op N:** same rollback path, with the verification mismatch
   surfaced.
4. **Snapshot failure at each step:** `failed`, zero mutations.
5. **Commit DB failure:** effects rolled back, transaction `rolled_back`, installation not
   marked applied.
6. **Rollback conflict:** a later transaction modifies a resource → `E_ROLLBACK_CONFLICT`,
   zero mutation, conflicts listed.
7. **Override path:** same conflict with `override_conflicts=true` → proceeds, records
   `conflict_override` history event.
8. **Rollback failure:** revert fails (permission denied, foreign edit) → the rollback
   transaction is `rollback_failed`, the original stays `committed`, the installation stays
   `applied`, the report names the resource, and there is no success claim.
9. **Rollback of rollback:** rolling back a rollback transaction returns to the state before
   the rollback, verified.
10. **Snapshot restore:** exact restored hashes/values, new transaction, drift warnings.
11. **Disk full during apply:** rollback attempted; outcome honest; no infinite retry.
12. **Permission change during apply:** operation fails with `E_IO_PERMISSION`; rollback
    runs; report includes the operation.
13. **File removed underneath apply:** operation fails or detects drift; rollback safe.
14. **Concurrent apply:** second call `E_BUSY`, first unaffected.
15. **Double-click apply:** only one transaction created (idempotency at command boundary).
16. **Consent mismatch / expired plan / plan hash mismatch:** refused before lock.
17. **Lock file stale after crash:** lock is released by the OS; recovery proceeds.
18. **No writes outside allowlisted roots:** `TreeSnapshot` of a sentinel home tree before
    and after shows only expected targets changed.
19. **Reverse idempotency:** calling `reverse` twice yields the same verified state.
20. **Journal ordering:** a spy verifies `applying` is committed before the executor call
    and `applied` after fsync.

### 4.6 Crash recovery

`CrashSimulator` kills the apply/rollback at every durable boundary:

- before `snapshot_complete`;
- after `snapshot_complete`, before the first op;
- before/after each op's `applying` and `applied` journal writes (2N points);
- during each op's filesystem write (simulated by aborting after temp write, before
  rename);
- after verification, before commit;
- after the commit transaction is durable but before the process responds (must be
  treated as committed; recovery completes nothing and never reverses it);
- during rollback, before/after each reversal;
- during recovery itself.

Expectations: startup recovery restores pre-state and verifies it; committed transactions
are never reversed; missing objects produce `recovery_required` with a report; no panic;
running recovery twice is safe.

### 4.7 KDE adapter

With `FakeBus` and a temp HOME:

- `KconfigFile` parsing: sections, nested groups, escaped `$e` values, immutable `[$i]`
  keys, localized variants, comments, blank lines, CRLF, duplicate keys (last wins),
  invalid lines preserved.
- Serializer preserves untouched lines byte-for-byte; only the targeted key changes; file
  mode preserved; atomic rename used.
- Immutable key write ⇒ `E_KEY_IMMUTABLE`; symlinked config file ⇒ `E_SYMLINK_TARGET`.
- Target map: every kind resolves to the exact path in [ARCHITECTURE.md](ARCHITECTURE.md)
  §6; pack ID cannot influence the path; installed name derivation is deterministic.
- Asset installers: icon theme requires `index.theme`; cursor theme requires `cursors/`;
  color scheme requires `.colors`; non-empty directories for plasma style/aurorae.
- KWin effects: correct `kwinrc [Plugins]` keys, reconfigure call issued, failure mapping.
- Shortcuts: correct action id, kglobalaccel call, read-back verification, failure
  rollback.
- Plasma scripts: generated text is byte-identical to the golden fixture for a given
  operation and value set; injection attempts in a path are rejected before generation;
  no template constructs `ConfigFile`; `currentConfigGroup` is always emitted before
  `readConfig`/`writeConfig`.
- Panels/widgets: property round-trip, create/remove with residue assertion, widget
  add/configure/remove with polling, layout restore by type and order.
- Look-and-feel: `contents/defaults` parsing, expansion into primitives, deleted-override
  capture, `kdedefaults` snapshot, and detection of a deliberately mixed configuration as
  "custom".
- Reload classifier: every capability maps to exactly one apply class; no `live` operation
  requires a restart; `systemctl` and `plasmashell --replace` never appear in any argv.
- Lost-write race: 20 parallel external writers to one config file; the engine must either
  observe every value or fail with `E_CONFIG_WRITE_RACE`, never report success.
- Version probe: exact argv, no shell, root-owned check, timeout, output regex, stderr
  ignored.
- Notifications: `KGlobalSettings` / KWin reconfigure calls use expected arguments.

Real-session suite (`#[ignore]`, enabled in CI on a disposable Plasma VM):

- Each of the fifteen capabilities against the oldest supported and latest stable Plasma 6
  minor in the release matrix.
- Apply → verify through the authoritative channel → rollback → original state verified by
  live state, effective config, and file hashes.
- **Never** restart plasmashell inside a test: it is rate-limited by systemd and can leave
  the session without a shell. Persisted state is verified by reading the file after the
  quiesce window, not by restarting.
- Quiesce test: mutate a panel property, assert the API reflects it immediately, assert
  `plasmashellrc` converges within the timeout, and assert the engine waits.
- Desync test: restore a cursor theme by file copy while KWin still holds the old theme;
  the adapter must use the forced two-step re-apply and end with live state == config.
- Unsupported reporting matches reality: panel opacity and cursor size are refused with
  their specific codes.
- Pixel evidence is supplementary only: assert change on a **static crop** (the panel
  strip was measured stable at RMSE 0 over 4 s). Full-screen diff assertions are
  prohibited as a pass criterion, because a live application dominates the diff
  (measured 0.18 between two states of the same theme).

### 4.8 Tauri commands

Each command is invoked directly (not through the webview) against a temp service:

- `get_app_info`, `get_system_status` (fake probe), `list_packs`, `get_pack_detail`.
- `install_local_pack`: valid, invalid, duplicate, path with non-UTF-8 components.
- `create_pack_plan`, `create_snapshot_restore_plan`, `discard_plan`,
  `apply_plan` (consent/hash/stale/expired), `rollback_transaction`
  (confirm_scope/override), `list_transactions`, `get_transaction_detail`,
  `list_snapshots`, `get_snapshot_detail`, `get_history`, `get_diagnostics`,
  `export_diagnostics`.
- Error mapping: every `ErrorCode` maps to the DTO shape expected by the UI, including
  `rollback` outcome and `recovery` report.
- Event payloads serialized exactly as the TS contract expects (`morphix://transaction-progress`,
  `-finished`, `recovery-required`).
- Commands reject unexpected fields and invalid IDs at the boundary.

### 4.9 React

Vitest + Testing Library, with `invoke` mocked at `api/client.ts`:

- System status: supported, unsupported reasons, root refusal, degraded adapter.
- Catalog: bundled/local badges, invalid pack surfaced, empty state.
- Pack detail: capability list, license/attribution, previews, compatibility.
- Preview: plan grouped by capability, old → new values, warnings, byte totals, restart
  notice; Apply disabled until consent checked and plan hash matches.
- Apply progress: staged events render, no layout thrash on rapid updates.
- Success/failure outcome: rollback succeeded vs rollback failed vs recovery required have
  distinct, unmistakable states.
- Transactions: list/detail, rollback scope dialog, conflict list, override requires typed
  confirmation phrase.
- Snapshots: list/detail, restore plan preview, drift warning.
- Local install: field-level validation errors, no success toast on failure.
- Navigation and accessibility: keyboard-only flows, focus management on dialogs, ARIA
  landmarks, contrast tokens checked in light/dark.
- XSS: pack name/description containing HTML renders escaped; no `dangerouslySetInnerHTML`.

### 4.10 End-to-end (VM)

Scripted drill on a disposable Plasma 6 VM (`tests/e2e/fresh-vm.sh`, planned):

1. Fresh user, no Morphix data.
2. Install package, launch, verify support verdict.
3. Apply each bundled pack, verify config keys/assets and live state, capture a static
   crop as supporting evidence.
4. Roll back each, verify hashes and live state match the pre-apply snapshot.
5. Restore an older snapshot, verify.
6. Kill the process mid-apply at a chosen operation, relaunch, verify recovery banner and
   restored state.
7. Unsupported scenario: Plasma 5 VM, verify read-only catalog and refusal.
8. Symlinked config scenario: verify `E_SYMLINK_TARGET` refusal, not a write outside home.

## 5. Required failure tests

These are mandatory, not optional coverage:

| Failure | Injection | Expected result |
| --- | --- | --- |
| Operation 3 of 8 fails | `FakeAdapter` fails at index 3 | Ops 1–2 reverted in reverse order, verified; `rolled_back`; report names op 3 |
| Disk unavailable/full | `FakeFileSystem` returns `ENOSPC` on write or snapshot object | Apply aborts, rollback attempted, outcome explicit; no partial committed state |
| File permissions changed | Remove write permission on target mid-test | Operation fails with `E_IO_PERMISSION`, rollback runs, report is honest |
| Snapshot corrupted | Delete/truncate a blob; edit manifest | `E_SNAPSHOT_CORRUPT`/`E_SNAPSHOT_UNREADABLE`; apply/restore refused before mutation |
| Unsupported Plasma version | Probe fixture 5.27.11 | `E_UNSUPPORTED_PLATFORM`, no plan |
| Traversal in pack | `../../` fixture | `E_PATH_TRAVERSAL` at validation, nothing written |
| Symlink escape | Symlink fixture and symlinked target | `E_SYMLINK_REJECTED`/`E_SYMLINK_TARGET`, nothing written outside root |
| Crash after op N | `CrashSimulator` at every boundary | Recovery restores pre-state and verifies; `recovered` transaction |
| Rollback interrupted | Crash during reversal | Next startup completes reversal or reports `recovery_required` |
| Foreign edit before rollback | Modify a target after apply | Conditional reverse refuses; `E_ROLLBACK_CONFLICT`; no overwrite |
| Missing DB | Delete DB file | No panic; read-only diagnostics; snapshot paths shown |
| Hostile TOML | Fuzz corpus | Typed error, no panic, bounded memory/time |
| Oversized input | 300 MiB sparse asset, 300 KiB manifest | Early rejection by size, no full read |
| Malicious description | HTML/script payload | Escaped text, CSP blocks execution |
| Root execution | `geteuid()==0` test double | `E_RUNNING_AS_ROOT`, no mutation |
| Concurrent apply | Two tasks | One succeeds/one gets `E_BUSY`; no interleaved journal |
| Commit failure | Inject SQLite error at commit | Filesystem rolled back; not marked applied |

## 6. Coverage and quality gates

- Every `ErrorCode` variant has at least one test that produces it.
- Every public operation kind has apply/verify/reverse tests.
- Every `OperationAction` variant is exercised through the transaction engine, not only in
  isolation.
- Parser and validator: every rejection rule in [PACK_SPEC.md](PACK_SPEC.md) §8 has a
  fixture.
- Line coverage target for `morphix-core`: ≥ 85%; branch coverage for
  `transactions`, `snapshots`, `packs::validator`, and `fs::sandbox`: ≥ 90%.
  Coverage is a signal, not a substitute for the failure tests above.
- No ignored tests in the default CI run except the documented, environment-gated KDE
  integration suite and benchmarks.
- Flaky tests are treated as defects: quarantine with an issue, fix within the phase.
- Fuzz targets run in CI for a bounded time (60 s each) on every PR, and longer nightly.

## 7. CI jobs (planned, once the workspace exists)

| Job | Commands |
| --- | --- |
| Rust | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` |
| Architecture | `cargo test -p morphix-core --test architecture` |
| Frontend | `npm ci`, `npm run typecheck`, `npm run test -- --run`, `npm run build` |
| Docs | internal link check, doc consistency check (`tests/check_docs.py` or equivalent) |
| Security | `cargo deny check`, `npm audit --omit=dev`, static process-spawn check |
| KDE VM | env-gated `cargo test --test kde_integration -- --ignored` |
| Nightly | fuzz targets, crash-boundary soak, recovery soak |

## 8. Manual verification checklists

Kept in `tests/manual/` once implementation starts:

- `fresh-install.md`: first launch, no network, support verdict, catalog.
- `apply-rollback.md`: per-pack apply, restart, visual check, rollback, diff.
- `crash-recovery.md`: kill -9 during apply at observed stages, relaunch, verify.
- `unsupported.md`: Plasma 5/other desktop/root/symlinked config.
- `accessibility.md`: keyboard-only, screen reader labels, contrast.

Manual results are recorded in the release checklist ([PHASES.md](PHASES.md) Phase 11),
not treated as a replacement for automated tests.

## 9. Test data and cleanup

- All temp trees are deleted on drop; on failure, the path is printed and retained for
  debugging (never inside the repository).
- Fixture packs are tiny (kilobytes) except the explicit oversize tests, which use sparse
  files generated at runtime.
- VM images are disposable; the test script creates a fresh user and destroys the VM
  afterwards. No test may run against the developer's session bus.
- Golden files (plans, generated wallpaper scripts, canonical JSON) are reviewed like code.
