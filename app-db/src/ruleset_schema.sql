CREATE TABLE change_planner_ruleset (
 version INTEGER PRIMARY KEY CHECK(version >= 1),
 ruleset_hash BLOB NOT NULL CHECK(length(ruleset_hash) = 32),
 hash_format TEXT NOT NULL CHECK(length(hash_format) > 0),
 class_table_version TEXT NOT NULL CHECK(length(class_table_version) > 0),
 known_location_table_version TEXT NOT NULL CHECK(length(known_location_table_version) > 0),
 rule_format_version INTEGER NOT NULL CHECK(rule_format_version >= 1),
 origin TEXT NOT NULL CHECK(origin IN ('gui', 'import', 'product_update')),
 change_summary TEXT NOT NULL CHECK(json_valid(change_summary)),
 body TEXT NOT NULL CHECK(json_valid(body)),
 created_at_ms INTEGER NOT NULL
) STRICT;
CREATE TRIGGER change_planner_ruleset_no_update BEFORE UPDATE ON change_planner_ruleset
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER change_planner_ruleset_no_delete BEFORE DELETE ON change_planner_ruleset
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER change_planner_ruleset_no_replace BEFORE INSERT ON change_planner_ruleset
WHEN EXISTS (SELECT 1 FROM change_planner_ruleset WHERE version = NEW.version)
BEGIN SELECT RAISE(ABORT, 'append_only'); END;
