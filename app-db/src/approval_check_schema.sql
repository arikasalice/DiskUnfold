CREATE TABLE job_runtime_approval_check (
 check_id BLOB PRIMARY KEY CHECK(length(check_id) = 16),
 job_id BLOB CHECK(job_id IS NULL OR length(job_id) = 16),
 request_version_id BLOB NOT NULL CHECK(length(request_version_id) = 16),
 request_kind TEXT NOT NULL CHECK(request_kind IN ('plan','restore','move_undo')),
 version_hash BLOB NOT NULL CHECK(length(version_hash) = 32),
 hash_format TEXT NOT NULL CHECK(length(hash_format) > 0),
 operation_kind TEXT NOT NULL CHECK(operation_kind IN ('trash','move','delete','restore','move_undo')),
 destination_summary TEXT CHECK(destination_summary IS NULL OR json_valid(destination_summary)),
 keep_summary TEXT CHECK(keep_summary IS NULL OR json_valid(keep_summary)),
 record_kind TEXT NOT NULL CHECK(record_kind IN ('start','finish')),
 check_result TEXT NOT NULL
  CHECK(check_result IN ('valid','not_issued','already_used','mismatch','confirmation_missing')),
 mismatch_fields TEXT NOT NULL DEFAULT '[]'
  CHECK(json_valid(mismatch_fields) AND json_type(mismatch_fields) = 'array'),
 end_state TEXT,
 approved_at_ms INTEGER NOT NULL,
 created_at_ms INTEGER NOT NULL,
 correlation_id BLOB NOT NULL CHECK(length(correlation_id) = 16),
 CHECK((record_kind = 'start' AND end_state IS NULL)
    OR (record_kind = 'finish' AND end_state IS NOT NULL AND length(end_state) > 0))
) STRICT;
CREATE INDEX ix_job_runtime_approval_check_request
 ON job_runtime_approval_check(request_version_id, created_at_ms, check_id);
CREATE INDEX ix_job_runtime_approval_check_job
 ON job_runtime_approval_check(job_id, created_at_ms, check_id);
CREATE TRIGGER job_runtime_approval_check_no_update BEFORE UPDATE ON job_runtime_approval_check
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_approval_check_no_delete BEFORE DELETE ON job_runtime_approval_check
WHEN COALESCE((SELECT open FROM app_db_purge_gate WHERE gate_key = 1), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_approval_check_no_replace BEFORE INSERT ON job_runtime_approval_check
WHEN EXISTS (SELECT 1 FROM job_runtime_approval_check WHERE check_id = NEW.check_id)
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
