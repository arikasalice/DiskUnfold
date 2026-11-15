-- F03 scan-completion observations: one append-only line of totals per finished GUI scan.
CREATE TABLE analysis_index_scope_observation (
 observation_id BLOB PRIMARY KEY CHECK(length(observation_id) = 16),
 scope_fingerprint BLOB NOT NULL CHECK(length(scope_fingerprint) = 32),
 fingerprint_format INTEGER NOT NULL CHECK(fingerprint_format >= 1),
 observed_at_ms INTEGER NOT NULL,
 job_id BLOB NOT NULL CHECK(length(job_id) = 16),
 origin TEXT NOT NULL CHECK(origin IN ('scan','incremental')),
 completeness TEXT NOT NULL CHECK(completeness IN ('complete','partial','missing','cancelled')),
 size_rule_id TEXT NOT NULL,
 size_rule_revision INTEGER NOT NULL CHECK(size_rule_revision >= 1),
 logical_state TEXT NOT NULL CHECK(logical_state IN ('exact','lower_bound','unknown')),
 logical_bytes INTEGER CHECK(logical_bytes IS NULL OR logical_bytes >= 0),
 logical_evidence TEXT NOT NULL,
 allocated_state TEXT NOT NULL CHECK(allocated_state IN ('exact','lower_bound','unknown')),
 allocated_bytes INTEGER CHECK(allocated_bytes IS NULL OR allocated_bytes >= 0),
 allocated_evidence TEXT NOT NULL,
 unique_physical_state TEXT NOT NULL CHECK(unique_physical_state IN ('exact','lower_bound','unknown')),
 unique_physical_bytes INTEGER CHECK(unique_physical_bytes IS NULL OR unique_physical_bytes >= 0),
 unique_physical_evidence TEXT NOT NULL,
 created_at_ms INTEGER NOT NULL,
 UNIQUE(job_id),
 CHECK((logical_state = 'unknown') = (logical_bytes IS NULL)),
 CHECK((allocated_state = 'unknown') = (allocated_bytes IS NULL)),
 CHECK((unique_physical_state = 'unknown') = (unique_physical_bytes IS NULL))
) STRICT;
CREATE INDEX ix_analysis_index_scope_observation_scope
 ON analysis_index_scope_observation(scope_fingerprint, fingerprint_format, observed_at_ms, observation_id);
CREATE TRIGGER analysis_index_scope_observation_no_update BEFORE UPDATE ON analysis_index_scope_observation
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER analysis_index_scope_observation_no_delete BEFORE DELETE ON analysis_index_scope_observation
WHEN COALESCE((SELECT open FROM app_db_purge_gate), 0) <> 1
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
