-- F21 link and state definitions used by terminal scan history.
CREATE TABLE job_runtime_job_link (
 job_id BLOB PRIMARY KEY CHECK(length(job_id) = 16),
 plan_version_id BLOB CHECK(plan_version_id IS NULL OR length(plan_version_id) = 16),
 plan_lineage_id BLOB CHECK(plan_lineage_id IS NULL OR length(plan_lineage_id) = 16),
 plan_hash BLOB CHECK(plan_hash IS NULL OR length(plan_hash) = 32),
 predecessor_job_id BLOB CHECK(predecessor_job_id IS NULL OR length(predecessor_job_id) = 16),
 relation_kind TEXT CHECK(relation_kind IN ('retry','restore','move_undo','effect_check')),
 record_origin TEXT NOT NULL CHECK(record_origin IN ('this_app','imported')),
 reckon_at_ms INTEGER NOT NULL,
 record_seq INTEGER NOT NULL CHECK(record_seq > 0),
 created_at_ms INTEGER NOT NULL,
 CHECK((predecessor_job_id IS NULL) = (relation_kind IS NULL))
) STRICT;
CREATE INDEX ix_job_runtime_job_link_reckon ON job_runtime_job_link(reckon_at_ms, record_seq);
CREATE UNIQUE INDEX ux_job_runtime_job_link_seq ON job_runtime_job_link(record_seq);
CREATE INDEX ix_job_runtime_job_link_pred ON job_runtime_job_link(predecessor_job_id);
CREATE TRIGGER job_runtime_job_link_no_update BEFORE UPDATE ON job_runtime_job_link
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_job_link_no_delete BEFORE DELETE ON job_runtime_job_link
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TABLE job_runtime_history_state (
 state_key TEXT PRIMARY KEY CHECK(state_key = 'history'),
 delete_generation INTEGER NOT NULL CHECK(delete_generation >= 0),
 last_record_seq INTEGER NOT NULL CHECK(last_record_seq >= 0),
 row_version INTEGER NOT NULL CHECK(row_version >= 1),
 updated_at_ms INTEGER NOT NULL
) STRICT;
INSERT INTO job_runtime_history_state VALUES ('history', 0, 0, 1, 0);
