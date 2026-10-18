# Morphix — Transaction and Rollback Model

- **Status:** Design baseline. Implemented in Phases 4–5.
- **Related:** [ARCHITECTURE.md](ARCHITECTURE.md) §7/§9, [DATABASE.md](DATABASE.md),
  [SECURITY.md](SECURITY.md), [TESTING.md](TESTING.md) §5.

This document defines the only path by which Morphix mutates user-visible state:
**plan → snapshot → apply → verify → commit**, with **rollback** on failure and **recovery**
after a crash.

---

## 1. Invariants

1. **No mutation without a plan.** Every committed operation was in a persisted,
   hash-addressed plan.
2. **No mutation without a snapshot.** The snapshot completion marker is durable before the
   first operation executes.
3. **No mutation without a journal entry.** An operation's intent is committed to SQLite
   before it runs; its completion is committed before the next operation starts.
4. **One mutation at a time.** An exclusive lock is held for the whole transaction.
5. **No false success.** A transaction is `committed` only after post-state verification.
   A rollback is `succeeded` only after restoration verification.
6. **Every operation is reversible.** An operation that cannot be reversed is rejected at
   plan time, before any mutation.
7. **No silent overwrite of newer changes.** Reversal is conditional on the state the
   transaction produced (or explicitly overridden by the user).
8. **The frontend never mutates.** Mutations occur only inside the Rust transaction engine.

## 2. Transaction kinds

| Kind | Created by | Effect |
| --- | --- | --- |
| `apply_pack` | `apply_plan` for a pack plan | Installs assets, writes settings, sets wallpaper |
| `restore_snapshot` | `apply_plan` for a snapshot-restore plan | Returns resources to a snapshot's recorded state |
| `rollback` | `rollback_transaction` | Reverses the effects of an earlier committed transaction |
| `recovery` | startup recovery | Completes reversal of an interrupted transaction (recorded in history, not as a new row) |

Rollback and snapshot restore create **new** committed transactions. The original
transaction is never rewritten; its result remains historical fact. A rollback transaction
takes a snapshot of the state it replaces, so **a rollback can itself be rolled back**.
This falls out of the model and is tested, but V0.1 does not surface a special UI for it
beyond the normal transaction history.

## 3. State machines

### 3.1 Transaction

```
                       +-------------+
                       | snapshotting|
                       +------+------+
                              | snapshot verified + durable
                              v
                       +-------------+     op failure      +------------+
                       |   applying  | ------------------> | rolling_back|
                       +------+------+                     +------+-----+
                              | all ops applied                   |
                              v                                   | verified
                       +-------------+     verify failure     v
                       |  verifying  | ------------------> +----------+
                       +------+------+                     |rolled_back|
                              | verified                   +----------+
                              v
                       +-------------+
                       |  committed  |
                       +-------------+

  pre-mutation failure (PREPARE or SNAPSHOT) ---> failed          (no mutation)
  reversal failure ---------------------------> rollback_failed   (recovery report)
  startup recovery cannot finish -------------> recovery_required (mutations blocked)
```

| State | Meaning | Mutations possible next |
| --- | --- | --- |
| `snapshotting` | Lock held; journal and snapshot being written | No |
| `applying` | At least one operation may have run | No |
| `verifying` | All operations applied; post-state being checked | No |
| `committed` | Verified; terminal | Rollback only |
| `rolling_back` | Reversal in progress | No |
| `rolled_back` | Reversal verified; terminal failure outcome for apply, success for rollback | — |
| `failed` | Failed before any mutation; terminal | — |
| `rollback_failed` | Reversal attempted and not verified; terminal, requires user action | Retry reversal |
| `recovery_required` | Startup recovery could not restore state; terminal until resolved | Manual/retry |

`applying`, `verifying`, `snapshotting`, and `rolling_back` are the only states a crash can
leave behind. Startup treats any of them as interrupted and drives recovery.

### 3.2 Operation

```
pending -> applying -> applied -> verifying -> verified
                                  |              |
                                  |              +--> reversing -> reverted
                                  +--> failed
```

`applying` means "intent recorded, outcome unknown" after a crash. Recovery treats
`applying` as *possibly executed* and reverses it (reversal is conditional and idempotent).

## 4. The plan

A plan is a persisted, immutable description of an intended change. It is produced without
writing to KDE state and is bound to preconditions so that it cannot be applied to a
changed system.

```rust
pub struct ChangePlan {
    pub id: PlanId,
    pub hash: PlanHash,                 // excludes id/created_at/expires_at
    pub kind: PlanKind,                 // ApplyPack | RestoreSnapshot
    pub pack: Option<PackRef>,          // id, version, content hash
    pub snapshot_source: Option<SnapshotId>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,          // default created_at + 10 min
    pub system: SystemFingerprint,      // plasma version, session, adapter probe
    pub capabilities: Vec<CapabilityGrant>,
    pub operations: Vec<PlannedOperation>,
    pub preconditions: Vec<Precondition>,
    pub warnings: Vec<PlanWarning>,
    pub totals: PlanTotals,             // bytes to install, op counts
    pub restart_required: bool,
}

pub struct PlannedOperation {
    pub index: u32,
    pub capability: Capability,
    pub target: ResolvedTarget,         // engine-resolved, never pack-supplied
    pub action: OperationAction,        // see §5
    pub previous: Option<ResourceState>,
    pub expected: ResourceState,
    pub description: String,            // user-facing, generated by engine
    pub verification: VerificationLevel,
}
```

Plan properties:

- **Deterministic:** same pack + same state ⇒ identical operations and identical
  `PlanHash`. Canonical JSON (sorted keys, fixed number formats) is hashed with SHA-256.
- **Self-contained:** applying needs only the plan row, the pack in the store, and the
  system state; no re-planning.
- **Expiring:** a stale-expired plan returns `E_PLAN_EXPIRED` before any lock is taken.
- **Read-only to create:** the planner uses read APIs only; `RecordingFileSystem` and a
  recording adapter assert zero writes during planning (MVP AC-10).

Preconditions:

| Precondition | Captured as |
| --- | --- |
| Pack content | SHA-256 of canonical manifest + checksum manifest |
| Plasma version compatibility | exact version + compatibility expression result |
| Adapter capability set | probe snapshot (wallpaper availability, interface presence) |
| Config file | SHA-256 of file bytes + per-key current value |
| Asset target | absent, or owned by this pack with tree hash |
| Shortcut | current binding for the action |
| KWin effect | current enabled state |
| Wallpaper | current descriptor for each targeted desktop |

At apply time, every precondition is re-evaluated inside the lock. Any mismatch returns
`E_PLAN_STALE` and writes nothing.

### 4.1 Plan warnings

Warnings do not block apply; they are displayed and acknowledged through the same consent:

- `manual_step` (wallpaper unsupported, panel guidance, restart needed)
- `restart_required` (Plasma style, some decorations)
- `overwrites_existing_value` (with old value shown; only for allowlisted keys already set)
- `dependency_applied` (dependency present, applied earlier)

Anything that could make the result unsafe is an error, not a warning.

## 5. Operations

Operations are a closed enum, not trait objects, so they can be serialized into the
journal, matched exhaustively, and reviewed in one place.

```rust
pub enum OperationAction {
    InstallAsset {
        kind: AssetKind,
        asset_key: String,
        installed_name: String,       // engine-derived
        target: PathBuf,              // resolved from the fixed target map
        tree_hash: Hash,              // expected content
    },
    WriteConfigValue {
        file: ConfigTarget,           // KdeGlobals | PlasmaRc | KWinRc
        group: String,
        key: String,
        value: ConfigValue,           // string | int | bool | string[]
    },
    WriteShortcut {
        component: String,
        action: String,
        keys: Vec<String>,
    },
    SetKWinEffect {
        id: String,
        enabled: bool,
    },
    SetWallpaper {
        pack_id: PackId,
        image_path: PathBuf,
        file_url: String,
    },
}
```

Mapping from the product brief's conceptual operations to the implemented set:
`CopyFile` and `InstallTheme` are realized by `InstallAsset` (per asset kind),
`WriteConfigValue` is `WriteConfigValue`, `SetWallpaper` is `SetWallpaper`,
`ModifyShortcut` is `WriteShortcut`, `EnableKWinEffect` is `SetKWinEffect`, and
`InstallPlasmaLayout` is **not** in V0.1 (ADR-011).

Each `PlannedOperation` carries the reverse data captured at plan time (`previous`) and
the expected post-state (`expected`). At apply time the executor records the actually
observed previous value before writing; if it differs from the plan, the transaction aborts
with `E_PLAN_STALE` before that operation mutates.

### 5.1 Operation contract

Every operation kind implements four behaviors:

| Behavior | Contract |
| --- | --- |
| `validate` | Pure checks before mutation: target is inside allowlisted roots, value passes the adapter allowlist, required KDE interface is available, reverse data is sufficient. Failure ⇒ plan is refused. |
| `apply` | Performs exactly the planned change, then fsyncs the affected file/dir. Idempotent when re-run after a crash only where safe. Returns observed prior state. |
| `verify` | Re-reads persisted state and compares with `expected`. Returns `StateVerified`/`LiveVerified`/`Manual` plus a mismatch description. Never mutates. |
| `reverse` | Restores the recorded prior state **conditionally**: if current state equals the state this transaction produced, restore; otherwise report a conflict and do not overwrite. Verifies the restoration and returns its outcome. |

Additional operation rules:

- **Self-contained notifications.** An operation that needs KDE to reload (KWin effects,
  shortcuts, wallpaper) performs its own DBus notification as part of apply and reverse.
  There is no separate "notify" operation whose reversal could be ambiguous.
- **No shell, no external binaries.** All operations use filesystem APIs or DBus.
- **No partial asset installs.** Assets are staged to a temp directory inside the target's
  parent, fsynced, then atomically renamed into place.
- **Ownership marker.** Installed asset directories contain a generated
  `.morphix-owner` file with the pack ID and asset key. It is part of `tree_hash` and is
  removed with the directory. It is the on-disk ownership record; SQLite has the index.
- **Modes.** Files are written `0644`, directories `0755` (minus umask), and executable
  bits are never set. Existing user-owned asset directories are never overwritten unless
  recorded as owned by the same pack at the same version.

### 5.2 Per-operation reverse semantics

| Operation | Apply | Reverse | Conflict if |
| --- | --- | --- | --- |
| `InstallAsset` | Copy tree/file into resolved target, write ownership marker | If tree hash matches installed hash, remove tree; if replacing an owned previous version, restore previous tree from snapshot | Target hash differs (someone edited it) |
| `WriteConfigValue` | Read-modify-write one key; preserve other lines; notify if needed | If current value equals written value, restore previous value or delete key if it was absent; notify | Current value differs (someone changed it) |
| `WriteShortcut` | Write config key + `kglobalaccel` sync; read back | If current binding equals written binding, restore previous binding and sync | Binding changed |
| `SetKWinEffect` | Write `kwinrc` key + reconfigure; read back | If current enabled state equals written state, restore previous and reconfigure | State changed |
| `SetWallpaper` | Install image, call constrained script, read back descriptor | If current descriptor equals written descriptor, apply previous descriptor | Wallpaper changed by user |

## 6. Snapshot

A snapshot is a durable, self-describing capture of every resource a transaction may
change.

### 6.1 Layout

```
~/.local/share/morphix/snapshots/<snapshot-id>/
├── snapshot.json          # manifest, canonical JSON, fsynced
└── objects/
    ├── <sha256>           # file blobs (content-addressed, deduplicated across snapshots)
    └── ...
```

`snapshot.json`:

```json
{
  "format_version": 1,
  "snapshot_id": "...",
  "transaction_id": "...",
  "created_at": "2026-10-18T09:00:00Z",
  "app_version": "0.1.0",
  "system": { "plasma_version": "6.3.5", "session": "wayland" },
  "entries": [
    {
      "resource": { "type": "config_key", "file": "kdeglobals", "group": "General", "key": "ColorScheme" },
      "pre_state": { "kind": "value", "value": "BreezeDark" },
      "object": null
    },
    {
      "resource": { "type": "file", "path": "/home/u/.config/kdeglobals" },
      "pre_state": { "kind": "file", "sha256": "...", "mode": 420, "size": 8123 },
      "object": "<sha256>"
    },
    {
      "resource": { "type": "asset_dir", "path": "/home/u/.local/share/icons/cupertino-icons" },
      "pre_state": { "kind": "absent" },
      "object": null
    },
    {
      "resource": { "type": "wallpaper", "desktop": "all" },
      "pre_state": { "kind": "wallpaper", "plugin": "org.kde.image", "image": "file:///..." },
      "object": null
    }
  ],
  "manifest_hash": "..."
}
```

Rules:

- Capture **whole files** for every config file that will be modified, in addition to
  per-key values. Whole files make manual recovery and snapshot restore possible; per-key
  values make conditional rollback precise.
- Directory assets: capture each file as an object plus relative path/mode metadata.
- Missing target (for example a new asset directory) is recorded as `absent`, not an error.
- The manifest is written last, with `manifest_hash` computed over canonical JSON excluding
  that field; readers verify it. A corrupted manifest is `E_SNAPSHOT_CORRUPT`.
- Objects are written temp-file → fsync → rename → fsync directory; the manifest follows
  the same path; then the journal records `snapshot_complete`. Snapshot verification reads
  back every object hash before `snapshot_complete` is written.
- Objects are deduplicated by hash across snapshots; identical pre-states share blobs.
- Snapshots are never pruned automatically in V0.1. Orphaned partial snapshots from a
  failed pre-mutation aborts are garbage-collected on startup.

### 6.2 Snapshot scope

Snapshot exactly the resources in the plan, plus:

- the whole config file for each `WriteConfigValue`/`WriteShortcut` target;
- the current wallpaper descriptor per targeted desktop;
- the whole target tree for an asset that will be replaced (owned previous version);
- the directory itself if it exists and will be replaced or removed.

Snapshot nothing else. Snapshotting the home directory is forbidden: it is slow, leaks
unrelated data into the store, and creates a false sense of full-system backup.

## 7. Apply algorithm

```
apply_plan(plan_id, plan_hash, consent):
  1. load plan; validate hash, expiry, kind, consent.plan_hash
  2. require consent.acknowledged == plan.capabilities (set equality)
  3. system support check; refuse if unsupported (E_UNSUPPORTED_PLATFORM)
  4. acquire mutation lock (flock, non-blocking)      -> E_BUSY if held
     preflight free space (snapshot estimate + install + 64 MiB) -> E_INSUFFICIENT_SPACE
  5. DB BEGIN IMMEDIATE; re-check no other active tx;
     insert transaction(state=snapshotting) and all planned operations (state=pending)
  6. drift check: re-evaluate every precondition       -> E_PLAN_STALE
  7. create snapshot; verify; fsync; mark snapshot_complete
  8. state=applying
  9. for op in plan.operations (ascending):
       journal op=applying (durable) before executing
       execute op (targeted write, fsync, notify)
       observe/verify immediate postcondition
       journal op=applied (durable)
 10. state=verifying; verify all ops
 11. state=committed; update pack installation / snapshot links; append history
 12. release lock
```

Any error from step 6 onward after a mutation has occurred enters `rollback` (§8). Errors in
steps 1–7 produce `failed` with no mutation; the partial snapshot is orphaned and GC'd.

### 7.1 Durability ordering

For each operation: write target file → fsync file → fsync parent dir → journal `applied`.
This ensures a journal entry claiming `applied` is never ahead of the persisted effect.
Notifications (DBus) happen after the durable write and before journaling `applied`; a
failed notification is an operation failure (the write is still reversed).

### 7.2 Commit

Commit is a single SQLite transaction that:

- sets transaction `committed`, `finished_at`;
- sets each operation `verified`;
- upserts `pack_installations` (`status=applied`, version, transaction id);
- records installed assets with their target paths and tree hashes;
- appends `history_events` (`transaction_committed`);
- links the snapshot to the transaction.

If the commit transaction fails, the transaction is not committed: the engine rolls back
the filesystem effects and records `rolled_back`. SQLite failure never yields a "probably
committed" state.

## 8. Rollback on failure

Triggered by any failure after the first mutation.

```
rollback(tx):
  1. state=rolling_back
  2. for op in applied operations, reverse order:
       journal reversing
       reverse(op)                     # conditional on produced state
       verify reversal
       journal reverted
  3. verify full pre-state (per resource, against snapshot entries)
  4. state=rolled_back; append history; release lock
```

Failure handling inside rollback:

- A reversal conflict (someone changed the value between our write and our reversal) stops
  rollback. The engine records which operations were reverted, which conflicted, and which
  were not attempted, then sets `rollback_failed`. It never overwrites the foreign change.
- If a reversal fails for I/O reasons, retry once, then record `rollback_failed` with the
  error. A `RecoveryReport` lists affected resources and snapshot references.
- If reversal of the reversal is needed (to return to the state before a partially
  completed rollback), the engine attempts to re-apply reverted operations in forward
  order. If that also fails, `recovery_required` and mutations are blocked.
- The UI must display "rollback failed" with the report. It may never say "restored" unless
  restoration verification passed.

## 9. Committed rollback

`rollback_transaction(transaction_id, confirm_scope, override_conflicts)`:

1. Require original state `committed`, else `E_TRANSACTION_NOT_ROLLBACKABLE`.
2. Acquire the mutation lock.
3. **Conflict scan** over every resource identity touched by the original transaction:
   - config keys, shortcut actions, KWin effects: is there a later committed transaction
     whose operation targets the same identity?
   - asset paths: does a later transaction own the same target, or does the current tree
     hash differ from what the original installed?
   - wallpaper: was it changed by a later transaction or by someone else?
   If conflicts exist and `override_conflicts == false`, return `E_ROLLBACK_CONFLICT` with
   the list; mutate nothing. If true, record the override in history and proceed.
4. Create a new `rollback` transaction (linked via `parent_transaction_id`) with its own
   snapshot of the *current* state (so the rollback is reversible).
5. Reverse operations in reverse order and verify restoration.
6. Commit the rollback transaction, then set the original transaction's
   `rollback_transaction_id` and the pack installation status to `rolled_back`, and append
   history.

If reversal or verification fails, the rollback transaction ends `rollback_failed`, the
original transaction stays `committed`, and the pack installation stays `applied`; the
failure report names what could not be restored. A retry creates a new rollback
transaction.

Why rollback skips the preview-plan step: its target state is the recorded pre-state, its
scope is fixed by the original transaction, and delaying reversal behind another plan
increases the window in which the desktop stays broken. The UI still shows the scope and
requires `confirm_scope`. Snapshot restore is *not* exempt: it can overwrite arbitrary
recorded values, so it always goes through a previewed plan.

A rollback transaction is a normal committed transaction and can itself be rolled back.
Its snapshot captures the state it replaced, so "undo the undo" is a standard rollback.

## 10. Snapshot restore

`create_snapshot_restore_plan(snapshot_id)`:

- Validate the snapshot manifest hash and every referenced object.
- Expand entries into operations: full-file restore for config files, tree replacement for
  directories, wallpaper descriptor re-application, and removal of paths recorded as
  `absent` that now exist and were created by Morphix (not user-created paths).
- Warn when current state differs from both the snapshot and the original post-state
  ("these files changed since the snapshot; restoring will overwrite them").
- Requires user consent with a plan hash, like any pack apply.

Restore verification compares the resulting state to the snapshot entries (hashes and
values). A restore is not "best effort".

## 11. Crash recovery

At startup, before any command that can mutate:

1. Acquire the mutation lock. If unavailable, another instance is running; recovery is
   skipped and the UI shows that instance as active.
2. Query transactions in `snapshotting`, `applying`, `verifying`, `rolling_back`.
3. For each, in order:
   - If no `snapshot_complete` marker: no mutation can have occurred. Mark `failed`;
     GC partial snapshot.
   - Else, reverse operations whose journal state is `applied` or `verified`, plus any
     `applying` operation (unknown outcome — reverse conditionally, since reverse is
     idempotent).
   - Verify restoration against the snapshot. On success: `rolled_back` with
     `recovered = true`, append history event `recovered_after_crash`.
   - On failure: `recovery_required`, write `~/.local/state/morphix/recovery/<tx>.json`,
     emit `morphix://recovery-required`, and block all mutations until acknowledged.
4. Plans older than expiry are GC'd. Snapshots from committed transactions are retained.

Additional rules:

- Recovery never trusts a journal state of `applied` without checking the current value; a
  reverse whose precondition does not match reports a conflict instead of overwriting.
- Recovery is idempotent: running it twice after a crash in recovery itself is safe.
- If the database is unreadable, no recovery is attempted; the app offers diagnostics and
  the snapshot directory path. It never guesses.
- Crash during `snapshotting` is always safe: no mutation happened.
- Crash after `commit`'s SQLite write is durable but before the process exits is treated as
  committed; recovery must not reverse a committed transaction.

The recovery state machine is tested with a crash simulator that kills the executor at
every operation boundary and at every journal write boundary (see [TESTING.md](TESTING.md)
§5.3).

## 12. Failure summary and reporting

Every apply/rollback result is a `TransactionOutcome`:

```rust
pub struct TransactionOutcome {
    pub transaction_id: TransactionId,
    pub state: TransactionState,          // committed | rolled_back | failed | ...
    pub operations: Vec<OperationOutcome>,// index, action, result, error
    pub rollback: RollbackOutcome,        // NotNeeded | Succeeded | Failed { report }
    pub recovery: Option<RecoveryReport>,
    pub warnings: Vec<PlanWarning>,
    pub finished_at: Timestamp,
}
```

Reporting rules:

- Failure messages name the failed operation and its target in user terms (for example
  "Setting the icon theme (kdeglobals [Icons] Theme)").
- The report always answers: what changed, what was reverted, what could not be reverted,
  and what the user should do next.
- `E_ROLLBACK_FAILED` includes snapshot paths so nothing is lost even if the app cannot
  repair state automatically.
- Rollback success requires verification; otherwise it is reported as failed.

## 13. Conflict and ownership rules

Resource identities used for conflict detection:

| Identity | Key |
| --- | --- |
| Config value | `(file, group, key)` |
| Asset | absolute target path |
| Shortcut | `(component, action)` |
| KWin effect | effect id |
| Wallpaper | desktop scope (`all` in V0.1) |

Rules:

- Two operations in the same plan may not target the same identity (`E_DUPLICATE_TARGET`).
- A plan that would overwrite an asset path not owned by the pack is refused
  (`E_TARGET_EXISTS`), never silently replaced.
- Rollback refuses to overwrite a newer change unless overridden.
- No operation deletes a path it did not create, except when restoring a snapshot that
  recorded that path as present.
- User edits made after an apply are preserved by conditional reversal; they surface as a
  conflict instead of being clobbered.

## 14. Retention and storage growth

- Every apply, rollback, and restore creates a snapshot. Content addressing deduplicates
  unchanged files across snapshots.
- V0.1 keeps all snapshots; the snapshots screen shows disk usage and the objects each
  snapshot references.
- Automatic pruning, manual deletion, and retention policies are post-MVP (see
  [MVP.md](MVP.md) §3). Pruning is never automatic because deleting the only copy of a
  pre-state silently removes the user's ability to recover.
- Diagnostic export includes sizes and counts, not contents.

## 15. Failure matrix

| Failure point | Mutations before failure | Outcome | User-visible result |
| --- | --- | --- | --- |
| Plan validation | none | `E_*`, no transaction row | Error with field/target |
| Consent mismatch | none | `E_CONSENT_REQUIRED` | Re-confirm |
| Expired/stale plan | none | `E_PLAN_EXPIRED` / `E_PLAN_STALE` | Re-preview |
| Lock held | none | `E_BUSY` | "Another change is in progress" |
| Snapshot write/read | none | `failed` | Error, no changes made |
| Operation N of M fails | 0..N-1 applied | `rolling_back` → `rolled_back` | "Change failed; previous state restored (verified)" |
| Verification fails | all applied | `rolling_back` → `rolled_back` | Same, with failing resource |
| Commit DB failure | all applied | rollback → `rolled_back` | "Could not commit; restored" |
| Rollback conflict | partial reversal possibly | `rollback_failed` or retry | "Could not fully restore; details" |
| Crash mid-apply | unknown | recovery → `rolled_back` | "Recovered unfinished change at startup" |
| Crash with missing blob | unknown | `recovery_required` | Blocked with recovery report |
| Disk full during snapshot | none | `failed` | "Not enough space; nothing changed" |
| Disk full during apply | partial | rollback attempted; may fail | Explicit rollback outcome |
| Plasma restarted mid-apply | partial | rollback attempted | Explicit outcome; adapter probe re-run |

## 16. Test obligations

Every rule in this document maps to a test in [TESTING.md](TESTING.md):

- plan determinism and purity;
- snapshot completeness and corruption detection;
- apply success, failure at every operation boundary, and commit failure;
- rollback success, conflict refusal, and override path;
- rollback of rollback;
- crash at every journal boundary, including inside recovery;
- restore from snapshot including drift warnings;
- lock contention and concurrent apply refusal;
- no writes outside allowlisted roots;
- verification is mandatory for any success claim.
