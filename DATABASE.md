# Morphix — Local Database Design

- **Status:** Design baseline; schema version 1, implemented in Phases 2 and 5.
- **Engine:** SQLite (via `rusqlite`, bundled SQLite), single local file.
- **Location:** `~/.local/share/morphix/db/morphix.sqlite3`
- **Related:** [ARCHITECTURE.md](ARCHITECTURE.md) §10, [TRANSACTIONS.md](TRANSACTIONS.md),
  [SECURITY.md](SECURITY.md) §11.

The database is the durable index of packs, plans, transactions, snapshots, and history. It
never stores asset blobs or snapshot file contents: those live in the content-addressed
snapshot store. It exists so that rollback and crash recovery are possible after the
process dies, and so history survives restarts.

---

## 1. Principles

1. **The journal is authoritative.** Transaction and operation state is committed to SQLite
   before filesystem mutations, with `PRAGMA synchronous=FULL`.
2. **Snapshots are authoritative for content.** The DB stores snapshot metadata and an
   entry index; `snapshot.json` remains readable without the DB.
3. **Do not store what can be derived.** Pack names, descriptions, assets, and config
   values are read from the pack store and system. Only integrity hashes and
   installation/ownership state are persisted.
4. **Append-only history.** `history_events` rows are never updated or deleted (enforced by
   triggers). Corrections are new events.
5. **Forward-only migrations.** No downgrade path; a backup is taken before migrating.
6. **One writer.** A single connection owned by `Storage` behind a mutex; WAL allows reads,
   but V0.1 serializes writes through the app and the mutation lock.
7. **No secrets.** No credentials exist. Paths are stored; file contents are not.

## 2. Entity relationships

```
packs 1---0..1 pack_installations        (a pack may be installed and possibly applied)
  |
  +---1..n installed_assets              (asset ownership per pack)
  |
  +---1..n transactions (pack_id)        (apply transactions for this pack)

transactions 1---1..n transaction_operations
transactions 1---0..1 pack_installations (applied_transaction_id)
transactions 1---0..n snapshots          (owning transaction)
transactions 1---0..1 transactions       (rollback links via rollback_transaction_id)
transactions 1---0..1 plans              (plan_id, logical reference)
snapshots 1---1..n snapshot_entries
history_events 0..n---1 transactions     (logical reference, no cascade)

plans (ephemeral)                        app_settings (key/value)
schema_migrations
```

## 3. Schema (version 1)

Timestamps are RFC 3339 UTC strings (`2026-10-18T09:00:00Z`). Booleans are `INTEGER 0|1`
with `CHECK` constraints. IDs are UUIDv7 strings except pack IDs, which are manifest IDs.

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = FULL;
PRAGMA busy_timeout = 5000;
PRAGMA user_version = 1;
-- The DDL below is grouped for readability. Foreign keys may reference tables that are
-- created later in the same migration; every table exists before any DML runs.

CREATE TABLE schema_migrations (
    version      INTEGER PRIMARY KEY,
    description  TEXT NOT NULL,
    applied_at   TEXT NOT NULL,
    checksum     TEXT NOT NULL
);

-- Installed and registered packs (bundled packs are registered at first launch).
CREATE TABLE packs (
    pack_id          TEXT PRIMARY KEY,
    version          TEXT NOT NULL,
    manifest_hash    TEXT NOT NULL,
    content_hash     TEXT NOT NULL,   -- pinned tree hash at install/registration
    source           TEXT NOT NULL CHECK (source IN ('bundled', 'local')),
    store_path       TEXT NOT NULL,
    integrity_source TEXT NOT NULL CHECK (integrity_source IN ('declared', 'generated_local')),
    status           TEXT NOT NULL DEFAULT 'valid' CHECK (status IN ('valid', 'invalid')),
    installed_at     TEXT NOT NULL,
    last_validated_at TEXT
);

-- Current applied state. One row per pack; history lives in transactions.
CREATE TABLE pack_installations (
    pack_id                TEXT PRIMARY KEY
                           REFERENCES packs(pack_id) ON DELETE RESTRICT,
    version                TEXT NOT NULL,
    status                 TEXT NOT NULL CHECK (status IN ('applied', 'rolled_back')),
    applied_transaction_id TEXT REFERENCES transactions(transaction_id) ON DELETE RESTRICT,
    applied_at             TEXT,
    updated_at             TEXT NOT NULL
);

CREATE TABLE transactions (
    transaction_id        TEXT PRIMARY KEY,
    kind                  TEXT NOT NULL
                          CHECK (kind IN ('apply_pack', 'rollback', 'restore_snapshot')),
    state                 TEXT NOT NULL
                          CHECK (state IN ('snapshotting', 'applying', 'verifying', 'committed',
                                           'rolling_back', 'rolled_back', 'failed',
                                           'rollback_failed', 'recovery_required')),
    pack_id               TEXT REFERENCES packs(pack_id) ON DELETE RESTRICT,
    snapshot_id           TEXT,                     -- logical reference; see snapshots
    plan_id               TEXT,
    plan_hash             TEXT,
    parent_transaction_id TEXT REFERENCES transactions(transaction_id) ON DELETE RESTRICT,
    rollback_transaction_id TEXT REFERENCES transactions(transaction_id) ON DELETE RESTRICT,
    override_conflicts    INTEGER NOT NULL DEFAULT 0 CHECK (override_conflicts IN (0, 1)),
    recovered             INTEGER NOT NULL DEFAULT 0 CHECK (recovered IN (0, 1)),
    failure_code          TEXT,
    failure_message       TEXT,
    app_version           TEXT NOT NULL,
    started_at            TEXT NOT NULL,
    finished_at           TEXT
);

CREATE TABLE transaction_operations (
    operation_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id    TEXT NOT NULL REFERENCES transactions(transaction_id) ON DELETE CASCADE,
    op_index          INTEGER NOT NULL,
    capability        TEXT NOT NULL,
    action_type       TEXT NOT NULL
                      CHECK (action_type IN ('install_asset', 'write_config_value',
                                             'write_shortcut', 'set_kwin_effect',
                                             'set_wallpaper')),
    resource_identity TEXT NOT NULL,
    target_path       TEXT,
    previous_json     TEXT,
    expected_json     TEXT,
    state             TEXT NOT NULL
                      CHECK (state IN ('pending', 'applying', 'applied', 'verifying', 'verified',
                                       'reversing', 'reverted', 'failed', 'skipped')),
    error_code        TEXT,
    error_message     TEXT,
    started_at        TEXT,
    finished_at       TEXT,
    UNIQUE (transaction_id, op_index)
);

CREATE TABLE snapshots (
    snapshot_id    TEXT PRIMARY KEY,
    transaction_id TEXT NOT NULL REFERENCES transactions(transaction_id) ON DELETE RESTRICT,
    kind           TEXT NOT NULL
                   CHECK (kind IN ('apply', 'rollback', 'restore')),
    state          TEXT NOT NULL CHECK (state IN ('partial', 'complete', 'corrupt')),
    manifest_path  TEXT NOT NULL,
    manifest_hash  TEXT NOT NULL,
    entry_count    INTEGER NOT NULL DEFAULT 0,
    total_bytes    INTEGER NOT NULL DEFAULT 0,
    app_version    TEXT NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE TABLE snapshot_entries (
    entry_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id    TEXT NOT NULL REFERENCES snapshots(snapshot_id) ON DELETE CASCADE,
    resource_type  TEXT NOT NULL
                   CHECK (resource_type IN ('config_file', 'config_key', 'asset_file',
                                            'asset_dir', 'wallpaper')),
    resource_path  TEXT NOT NULL,
    group_name     TEXT NOT NULL DEFAULT '',
    key_name       TEXT NOT NULL DEFAULT '',
    object_hash    TEXT,
    pre_state_json TEXT NOT NULL,
    UNIQUE (snapshot_id, resource_type, resource_path, group_name, key_name)
);

-- Persisted only while a plan may be applied; GC'd after apply/expiry.
CREATE TABLE plans (
    plan_id      TEXT PRIMARY KEY,
    kind         TEXT NOT NULL CHECK (kind IN ('apply_pack', 'restore_snapshot')),
    pack_id      TEXT,
    snapshot_id  TEXT,
    plan_hash    TEXT NOT NULL,
    plan_json    TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    consumed_at  TEXT,
    discarded_at TEXT
);

CREATE TABLE installed_assets (
    asset_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    pack_id        TEXT NOT NULL REFERENCES packs(pack_id) ON DELETE RESTRICT,
    asset_kind     TEXT NOT NULL
                   CHECK (asset_kind IN ('color_scheme', 'icon_theme', 'cursor_theme',
                                         'plasma_style', 'window_decoration', 'wallpaper')),
    asset_key      TEXT NOT NULL,
    target_path    TEXT NOT NULL UNIQUE,
    is_directory   INTEGER NOT NULL CHECK (is_directory IN (0, 1)),
    tree_hash      TEXT NOT NULL,
    transaction_id TEXT NOT NULL REFERENCES transactions(transaction_id) ON DELETE RESTRICT,
    installed_at   TEXT NOT NULL,
    UNIQUE (pack_id, asset_kind, asset_key)
);

CREATE TABLE history_events (
    event_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at   TEXT NOT NULL,
    event_type    TEXT NOT NULL
                  CHECK (event_type IN ('transaction_started', 'transaction_committed',
                                        'transaction_failed', 'transaction_rolled_back',
                                        'rollback_succeeded', 'rollback_failed',
                                        'snapshot_created', 'snapshot_restored',
                                        'recovery_completed', 'recovery_required',
                                        'pack_installed', 'pack_install_rejected',
                                        'conflict_override')),
    transaction_id TEXT,
    pack_id        TEXT,
    summary        TEXT NOT NULL,
    details_json   TEXT
);

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Indexes for the known query patterns.
CREATE INDEX idx_transactions_started        ON transactions (started_at DESC);
CREATE INDEX idx_transactions_state          ON transactions (state);
CREATE INDEX idx_transactions_pack           ON transactions (pack_id, started_at DESC);
CREATE INDEX idx_transactions_plan           ON transactions (plan_id);
CREATE INDEX idx_ops_transaction             ON transaction_operations (transaction_id, op_index);
CREATE INDEX idx_ops_resource                ON transaction_operations (resource_identity, transaction_id);
CREATE INDEX idx_ops_state                   ON transaction_operations (state);
CREATE INDEX idx_snapshots_created           ON snapshots (created_at DESC);
CREATE INDEX idx_snapshots_transaction       ON snapshots (transaction_id);
CREATE INDEX idx_snapshot_entries_resource   ON snapshot_entries (resource_path);
CREATE INDEX idx_plans_expires               ON plans (expires_at);
CREATE INDEX idx_assets_pack                 ON installed_assets (pack_id);
CREATE INDEX idx_history_occurred            ON history_events (occurred_at DESC);
CREATE INDEX idx_history_transaction         ON history_events (transaction_id);
CREATE INDEX idx_pack_installations_status   ON pack_installations (status);

-- History is append-only.
CREATE TRIGGER history_events_no_update
BEFORE UPDATE ON history_events
BEGIN
    SELECT RAISE(ABORT, 'history_events is append-only');
END;

CREATE TRIGGER history_events_no_delete
BEFORE DELETE ON history_events
BEGIN
    SELECT RAISE(ABORT, 'history_events is append-only');
END;
```

Note: `packs` is created before `pack_installations`, which references `transactions`;
`transactions` is created in between in the migration file. `pack_installations.applied_transaction_id`
is nullable so registration of a never-applied pack does not violate the foreign key.

## 4. Table purposes

| Table | Purpose | Growth |
| --- | --- | --- |
| `schema_migrations` | Applied migration versions and checksums | Bounded |
| `packs` | Registered packs, version, manifest hash, pinned content hash, source, integrity provenance | One row per installed/bundled pack |
| `pack_installations` | Whether a pack is currently applied and by which transaction | One row per applied pack |
| `transactions` | Transaction lifecycle and outcome, including recovery flags | One row per apply/rollback/restore |
| `transaction_operations` | Per-operation journal, reverse data, conflicts | O(operations × transactions) |
| `snapshots` | Snapshot index and integrity metadata | One per transaction |
| `snapshot_entries` | Queryable index of captured resources (manifest is authoritative) | O(resources × snapshots) |
| `plans` | Ephemeral persisted plans for apply | Bounded by active plans |
| `installed_assets` | Ownership of installed targets, prevents overwrite/ambiguous delete | One row per installed asset |
| `history_events` | User-facing audit trail | Append-only |
| `app_settings` | UI preferences, onboarding flags, last launch version | Bounded |

## 5. Invariants enforced in code and SQL

1. At most one transaction in an active state (`snapshotting`, `applying`, `verifying`,
   `rolling_back`) — enforced by the mutation lock and checked in the insert transaction.
2. `pack_id` is unique in `packs`; V0.1 has one version per pack ID.
3. `installed_assets.target_path` is unique across all packs.
4. A `committed` transaction has `finished_at`, and all non-skipped operations are
   `verified`.
5. A `rolled_back` transaction has no operations in `applied`/`verified` state.
6. `rollback_transaction_id` points to a transaction of kind `rollback` whose
   `parent_transaction_id` points back to the original.
7. `snapshots.manifest_hash` matches `snapshot.json` at read time; mismatch sets
   `state='corrupt'` and blocks restore.
8. `plans.expires_at > plans.created_at`; consumed and discarded plans are mutually
   exclusive.
9. History events are never updated or deleted (trigger).
10. No operation row is written for a plan that failed validation: failed plans do not
    create transactions.

## 6. Query patterns

```
Startup recovery:
  SELECT * FROM transactions
  WHERE state IN ('snapshotting','applying','verifying','rolling_back')
  ORDER BY started_at;

Catalog applied state:
  SELECT p.pack_id, p.version, i.status, i.applied_at
  FROM packs p LEFT JOIN pack_installations i USING (pack_id);

Transaction detail:
  SELECT * FROM transactions WHERE transaction_id = ?;
  SELECT * FROM transaction_operations WHERE transaction_id = ? ORDER BY op_index;

Rollback conflict scan:
  SELECT o2.resource_identity, t2.transaction_id, t2.started_at
  FROM transaction_operations o1
  JOIN transactions t1        ON t1.transaction_id = o1.transaction_id
  JOIN transaction_operations o2 ON o2.resource_identity = o1.resource_identity
  JOIN transactions t2        ON t2.transaction_id = o2.transaction_id
  WHERE t1.transaction_id = ?
    AND t2.state = 'committed'
    AND t2.started_at > t1.started_at
    AND t2.kind IN ('apply_pack','restore_snapshot');

History page:
  SELECT * FROM history_events ORDER BY occurred_at DESC, event_id DESC LIMIT ? OFFSET ?;

Expired plans GC:
  DELETE FROM plans WHERE expires_at < ? AND consumed_at IS NULL;
```

All statements are parameterized; no pack-provided string is ever interpolated into SQL.

## 7. Connection and transaction policy

- One `Storage` owns one `rusqlite::Connection`; commands that need DB access go through
  the `MorphixService` worker, not concurrent connections.
- `PRAGMA journal_mode=WAL`, `foreign_keys=ON`, `synchronous=FULL`, `busy_timeout=5000`.
  `FULL` is chosen because journal durability before mutation matters more than write
  throughput; applies are rare and small.
- Every journal state change is its own short write transaction, committed before the
  corresponding filesystem action.
- Plan creation is a single insert.
- Commit/upsert of installation and assets happens in one transaction with the final state
  change and history event.
- No long-running read transaction is held while waiting on filesystem I/O.

## 8. Migrations

- Migration files live in `crates/morphix-core/src/storage/migrations/` and are embedded
  with `include_str!`.
- File naming: `0001_init.sql`, `0002_<description>.sql`, sequential, never edited after
  release.
- On startup:
  1. Open the DB; run `PRAGMA quick_check` (bounded, small database). Failure ⇒ read-only
     diagnostics mode; mutation commands return `E_DB_CORRUPT`.
  2. Read `schema_migrations`; if the DB is newer than the app (`user_version > 1`), refuse
     to mutate (`E_DB_NEWER_THAN_APP`).
  3. If migrations are pending, copy the DB file to
     `db/backup-v<current>-<timestamp>.sqlite3` (mode 0600), then apply each migration in a
     single transaction and insert its row with a SHA-256 checksum of the SQL.
- Downgrades are not supported. Users who downgrade the app see a clear error rather than a
  silent partial schema.
- A migration must never drop user-visible history. Destructive transformations require a
  new table plus copy, and an ADR.

## 9. Corruption, backup, and recovery

- The DB is small (metadata only); snapshot content is outside it.
- `E_DB_CORRUPT` blocks mutations but the app still lists snapshots from disk and provides
  their paths in diagnostics.
- If the DB is lost entirely, snapshot manifests remain self-describing. Rebuilding the DB
  from disk is *not* implemented in V0.1; the user can still recover files manually from the
  snapshot store documented in [TRANSACTIONS.md](TRANSACTIONS.md) §6.
- Deleting the DB never deletes snapshot objects; it only loses history and ownership
  indexing (which is why the app never offers "reset database" without an explicit warning
  that rollback history will be lost).

## 10. Retention

| Data | V0.1 policy |
| --- | --- |
| `plans` | Deleted on consume/discard; expired plans GC'd at startup and after each apply |
| `transactions` / `transaction_operations` | Retained indefinitely (history) |
| `snapshots` / objects | Retained indefinitely; never auto-pruned |
| `history_events` | Retained indefinitely; append-only |
| `packs` | Removed only if a pack is uninstalled (post-MVP) and never applied |
| `app_settings` | Bounded keys; no growth |

Automatic pruning is deliberately deferred: deleting a snapshot silently removes the
ability to recover. Users see snapshot disk usage in the UI.

## 11. Estimated size

| Data | Estimate |
| --- | --- |
| Transaction + operations | ~2–10 KB per transaction |
| Snapshot metadata | ~50–500 KB per snapshot (index + manifest) |
| Snapshot objects | Dominated by assets and config files actually captured; deduplicated |
| DB total after 100 transactions | Single-digit MB |

## 12. Settings keys (`app_settings`)

| Key | Type | Purpose |
| --- | --- | --- |
| `ui.theme` | string (`system`/`light`/`dark`) | UI appearance only |
| `ui.onboarded` | boolean | First-run completion |
| `log.level` | string (`info`/`debug`/`warn`) | Local log verbosity |
| `last.launch_version` | string | Show "what's new" / migration notice |
| `consent.diagnostics_exported_once` | boolean | One-time reminder, never auto-upload |

No setting grants additional capabilities, and none can weaken validation.
