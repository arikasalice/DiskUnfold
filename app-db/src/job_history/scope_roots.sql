-- Stored text is the display value at recording time; non-UTF-8 names may be lossy.
CREATE TABLE job_runtime_job_scope_root (
 job_id BLOB NOT NULL CHECK(length(job_id)=16),
 root_seq INTEGER NOT NULL CHECK(root_seq BETWEEN 0 AND 99),
 location_text TEXT NOT NULL CHECK(length(location_text) BETWEEN 1 AND 32768),
 created_at_ms INTEGER NOT NULL,
 PRIMARY KEY(job_id, root_seq)
) STRICT;

CREATE TRIGGER job_runtime_job_scope_root_no_update BEFORE UPDATE ON job_runtime_job_scope_root
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER job_runtime_job_scope_root_no_delete BEFORE DELETE ON job_runtime_job_scope_root
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
