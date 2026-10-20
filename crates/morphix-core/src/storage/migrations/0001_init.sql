-- Morphix schema version 1 (DATABASE.md §3).
--
-- This file is immutable once released. The runner wraps it in one transaction and records
-- its SHA-256 in `schema_migrations`. Connection pragmas (WAL, foreign keys, synchronous,
-- busy timeout) are applied by `storage::db` before this file runs; `journal_mode` cannot
-- be changed inside a transaction on every SQLite build.

PRAGMA user_version = 1;

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
    content_hash     TEXT NOT NULL,
    source           TEXT NOT NULL CHECK (source IN ('bundled', 'local')),
    store_path       TEXT NOT NULL,
    integrity_source TEXT NOT NULL CHECK (integrity_source IN ('declared', 'generated_local')),
    status           TEXT NOT NULL DEFAULT 'valid' CHECK (status IN ('valid', 'invalid')),
    installed_at     TEXT NOT NULL,
    last_validated_at TEXT
);

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
    snapshot_id           TEXT,
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
