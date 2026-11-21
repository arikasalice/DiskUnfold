CREATE TABLE change_planner_plan_version (
 plan_version_id BLOB PRIMARY KEY CHECK(length(plan_version_id) = 16),
 lineage_id BLOB NOT NULL CHECK(length(lineage_id) = 16),
 parent_plan_version_id BLOB CHECK(parent_plan_version_id IS NULL OR length(parent_plan_version_id) = 16),
 version_hash BLOB NOT NULL CHECK(length(version_hash) = 32),
 hash_format TEXT NOT NULL CHECK(length(hash_format) > 0),
 ruleset_version INTEGER NOT NULL CHECK(ruleset_version >= 1)
  REFERENCES change_planner_ruleset(version) ON DELETE RESTRICT,
 readiness TEXT NOT NULL CHECK(readiness IN ('ready','needs_confirmation','no_target')),
 origin_channel TEXT NOT NULL CHECK(origin_channel IN ('gui','cli','mcp')),
 body TEXT NOT NULL CHECK(json_valid(body)),
 created_at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_change_planner_plan_version_lineage
 ON change_planner_plan_version(lineage_id, created_at_ms);
CREATE TRIGGER change_planner_plan_version_no_update BEFORE UPDATE ON change_planner_plan_version
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER change_planner_plan_version_no_delete BEFORE DELETE ON change_planner_plan_version
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER change_planner_plan_version_no_replace BEFORE INSERT ON change_planner_plan_version
WHEN EXISTS (SELECT 1 FROM change_planner_plan_version WHERE plan_version_id = NEW.plan_version_id)
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
