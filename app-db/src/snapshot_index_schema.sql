-- F07 snapshot index, baseline and append-only deletion records.
CREATE TABLE snapshot_store_snapshot (
 snapshot_id BLOB PRIMARY KEY CHECK(length(snapshot_id) = 16),
 file_name TEXT NOT NULL,
 source_version_id BLOB NOT NULL CHECK(length(source_version_id) = 16),
 lineage_id BLOB NOT NULL CHECK(length(lineage_id) = 16),
 source_kind TEXT NOT NULL CHECK(source_kind IN ('local','imported')),
 observed_at_ms INTEGER CHECK(observed_at_ms IS NULL OR observed_at_ms >= 0),
 created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
 scope_label BLOB NOT NULL CHECK(length(scope_label) IN (0,32)),
 condition_summary TEXT NOT NULL
     CHECK(json_valid(condition_summary) AND length(CAST(condition_summary AS BLOB)) <= 65536),
 completeness TEXT CHECK(completeness IS NULL OR completeness IN ('complete','partial','missing')),
 format_version INTEGER NOT NULL CHECK(format_version BETWEEN 1 AND 65535),
 body_size INTEGER NOT NULL CHECK(body_size >= 0),
 body_hash BLOB NOT NULL CHECK(length(body_hash) = 32),
 state TEXT NOT NULL CHECK(state IN ('active','deleted','removed')),
 verify_failed_at_ms INTEGER CHECK(verify_failed_at_ms IS NULL OR verify_failed_at_ms >= 0),
 updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
 row_version INTEGER NOT NULL CHECK(row_version >= 1)
) STRICT;

CREATE TRIGGER snapshot_store_snapshot_no_delete BEFORE DELETE ON snapshot_store_snapshot
BEGIN SELECT RAISE(ABORT, 'snapshot_state_only'); END;

CREATE UNIQUE INDEX ux_snapshot_store_snapshot_active_source
 ON snapshot_store_snapshot(source_version_id) WHERE state = 'active';
CREATE INDEX ix_snapshot_store_snapshot_list
 ON snapshot_store_snapshot(state, observed_at_ms, scope_label, snapshot_id);
CREATE INDEX ix_snapshot_store_snapshot_list_order
 ON snapshot_store_snapshot(state, observed_at_ms DESC, snapshot_id ASC);

CREATE TABLE snapshot_store_snapshot_volume (
 snapshot_id BLOB NOT NULL REFERENCES snapshot_store_snapshot(snapshot_id),
 volume_index INTEGER NOT NULL CHECK(volume_index BETWEEN 0 AND 65534),
 observed_volume_id BLOB,
 identity_kind TEXT NOT NULL CHECK(identity_kind = 'none'),
 identity_capability TEXT NOT NULL CHECK(identity_capability = 'unknown'),
 PRIMARY KEY(snapshot_id, volume_index)
) STRICT;
-- This index stays empty until identity values are allowed by the schema.
CREATE INDEX ix_snapshot_store_snapshot_volume_identity
 ON snapshot_store_snapshot_volume(identity_kind, observed_volume_id, snapshot_id)
 WHERE identity_kind <> 'none' AND observed_volume_id IS NOT NULL AND length(observed_volume_id) > 0;

CREATE TABLE snapshot_store_baseline (
 snapshot_id BLOB PRIMARY KEY REFERENCES snapshot_store_snapshot(snapshot_id),
 pinned_at_ms INTEGER NOT NULL CHECK(pinned_at_ms >= 0)
) STRICT;

CREATE TABLE snapshot_store_deletion (
 deletion_id BLOB PRIMARY KEY CHECK(length(deletion_id) = 16),
 snapshot_id BLOB NOT NULL REFERENCES snapshot_store_snapshot(snapshot_id),
 deleted_at_ms INTEGER NOT NULL CHECK(deleted_at_ms >= 0),
 cause TEXT NOT NULL CHECK(cause IN ('manual','swept')),
 file_removed INTEGER NOT NULL CHECK(file_removed IN (0,1))
) STRICT;
CREATE INDEX ix_snapshot_store_deletion_snapshot
 ON snapshot_store_deletion(snapshot_id);
CREATE TRIGGER snapshot_store_deletion_no_update BEFORE UPDATE ON snapshot_store_deletion
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER snapshot_store_deletion_no_delete BEFORE DELETE ON snapshot_store_deletion
BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TABLE snapshot_store_list_state (
 name TEXT PRIMARY KEY NOT NULL CHECK(name = 'snapshot_list'),
 list_seq INTEGER NOT NULL CHECK(list_seq >= 0)
) STRICT;
INSERT INTO snapshot_store_list_state(name, list_seq) VALUES ('snapshot_list', 0);
