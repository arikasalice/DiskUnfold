-- F24 terminal summary definition. The app-db purge gate must already exist.
CREATE TABLE job_runtime_job_record (
 job_id BLOB PRIMARY KEY CHECK(length(job_id) = 16),
 kind_id TEXT NOT NULL,
 kind_version INTEGER NOT NULL CHECK(kind_version >= 1),
 scope_summary TEXT NOT NULL,
 final_state TEXT NOT NULL CHECK(final_state IN ('done','partial','failed','cancelled')),
 completed_count INTEGER NOT NULL CHECK(completed_count >= 0),
 overwritten_count INTEGER NOT NULL CHECK(overwritten_count >= 0),
 failed_count INTEGER NOT NULL CHECK(failed_count >= 0),
 skipped_count INTEGER NOT NULL CHECK(skipped_count >= 0),
 unprocessed_count INTEGER NOT NULL CHECK(unprocessed_count >= 0),
 failure_reason_key TEXT,
 unprocessed_summary TEXT,
 result_version_id BLOB CHECK(result_version_id IS NULL OR length(result_version_id) = 16),
 started_at_ms INTEGER,
 finished_at_ms INTEGER NOT NULL,
 correlation_id BLOB NOT NULL CHECK(length(correlation_id) = 16),
 created_at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_job_runtime_job_record_finished ON job_runtime_job_record(finished_at_ms);
CREATE TRIGGER job_runtime_job_record_no_update BEFORE UPDATE ON job_runtime_job_record
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_job_record_no_delete BEFORE DELETE ON job_runtime_job_record
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
