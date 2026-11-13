-- F21 history schema and backfill from the preceding F24 summary schema.
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

CREATE TABLE job_runtime_job_start (
 job_id BLOB PRIMARY KEY CHECK(length(job_id) = 16),
 kind_id TEXT NOT NULL,
 kind_version INTEGER NOT NULL CHECK(kind_version >= 1),
 scope_summary TEXT NOT NULL,
 started_at_ms INTEGER NOT NULL,
 plan_version_id BLOB CHECK(plan_version_id IS NULL OR length(plan_version_id) = 16),
 plan_lineage_id BLOB CHECK(plan_lineage_id IS NULL OR length(plan_lineage_id) = 16),
 plan_hash BLOB CHECK(plan_hash IS NULL OR length(plan_hash) = 32),
 predecessor_job_id BLOB CHECK(predecessor_job_id IS NULL OR length(predecessor_job_id) = 16),
 relation_kind TEXT CHECK(relation_kind IN ('retry','restore','move_undo','effect_check')),
 record_origin TEXT NOT NULL CHECK(record_origin IN ('this_app','imported')),
 created_at_ms INTEGER NOT NULL,
 CHECK((predecessor_job_id IS NULL) = (relation_kind IS NULL))
) STRICT;
CREATE TRIGGER job_runtime_job_start_no_update BEFORE UPDATE ON job_runtime_job_start
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_job_start_no_delete BEFORE DELETE ON job_runtime_job_start
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TABLE job_runtime_job_interrupt (
 job_id BLOB PRIMARY KEY CHECK(length(job_id) = 16),
 detected_at_ms INTEGER NOT NULL
) STRICT;
CREATE TRIGGER job_runtime_job_interrupt_no_update BEFORE UPDATE ON job_runtime_job_interrupt
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_job_interrupt_no_delete BEFORE DELETE ON job_runtime_job_interrupt
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TABLE job_runtime_item_result (
 job_id BLOB NOT NULL CHECK(length(job_id) = 16),
 item_seq INTEGER NOT NULL CHECK(item_seq >= 0),
 item_ref BLOB NOT NULL CHECK(length(item_ref) > 0),
 location_raw BLOB NOT NULL,
 location_text TEXT NOT NULL,
 outcome TEXT NOT NULL CHECK(outcome IN ('completed','overwritten','skipped','failed','unprocessed')),
 reason_key TEXT,
 review_needed INTEGER NOT NULL CHECK(review_needed IN (0, 1)),
 held INTEGER NOT NULL CHECK(held IN (0, 1)),
 outcome_unknown INTEGER NOT NULL CHECK(outcome_unknown IN (0, 1)),
 observed_size INTEGER CHECK(observed_size >= 0),
 observed_size_state TEXT NOT NULL CHECK(observed_size_state IN ('exact','lower_bound','unknown')),
 displaced_to_trash INTEGER NOT NULL DEFAULT 0 CHECK(displaced_to_trash IN (0, 1)),
 displaced_trash_ref BLOB,
 displaced_name TEXT CHECK(displaced_name IS NULL OR length(displaced_name) > 0),
 displaced_original_location TEXT CHECK(displaced_original_location IS NULL OR length(displaced_original_location) > 0),
 displaced_trashed_at_ms INTEGER,
 created_at_ms INTEGER NOT NULL,
 PRIMARY KEY(job_id, item_seq),
 CHECK((observed_size IS NULL) = (observed_size_state = 'unknown')),
 -- The executor may observe displacement even when all its identifiers are unknown.
 CHECK(
  (displaced_to_trash = 1 AND outcome IN ('overwritten','failed'))
  OR (displaced_to_trash = 0 AND outcome <> 'overwritten'
      AND displaced_trash_ref IS NULL AND displaced_name IS NULL
      AND displaced_original_location IS NULL AND displaced_trashed_at_ms IS NULL)
 )
) STRICT;
CREATE TRIGGER job_runtime_item_result_no_update BEFORE UPDATE ON job_runtime_item_result
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_item_result_no_delete BEFORE DELETE ON job_runtime_item_result
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;

CREATE TABLE job_runtime_history_state (
 state_key TEXT PRIMARY KEY CHECK(state_key = 'history'),
 delete_generation INTEGER NOT NULL CHECK(delete_generation >= 0),
 last_record_seq INTEGER NOT NULL CHECK(last_record_seq >= 0),
 row_version INTEGER NOT NULL CHECK(row_version >= 1),
 updated_at_ms INTEGER NOT NULL
) STRICT;

INSERT INTO job_runtime_job_link
 (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
SELECT job_id, 'this_app', finished_at_ms,
       row_number() OVER (ORDER BY finished_at_ms, job_id), created_at_ms
FROM job_runtime_job_record;

INSERT INTO job_runtime_history_state
SELECT 'history', 0, count(*), 1, COALESCE(max(created_at_ms), 0)
FROM job_runtime_job_link;
