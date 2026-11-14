CREATE TABLE analysis_index_group_ruleset (
 ruleset_id BLOB PRIMARY KEY CHECK(length(ruleset_id) = 16),
 format_version INTEGER NOT NULL CHECK(format_version >= 1),
 version_no INTEGER NOT NULL CHECK(version_no >= 1),
 revision INTEGER NOT NULL CHECK(revision >= 1),
 fingerprint BLOB NOT NULL CHECK(length(fingerprint) = 32),
 created_at_ms INTEGER NOT NULL,
 updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE analysis_index_group_rule (
 rule_row_id BLOB PRIMARY KEY CHECK(length(rule_row_id) = 16),
 ruleset_id BLOB NOT NULL REFERENCES analysis_index_group_ruleset(ruleset_id) ON DELETE RESTRICT,
 group_id BLOB NOT NULL CHECK(length(group_id) = 16),
 display_name TEXT NOT NULL,
 display_order INTEGER NOT NULL CHECK(display_order >= 0),
 condition_json TEXT NOT NULL CHECK(json_valid(condition_json)),
 condition_model_version INTEGER NOT NULL CHECK(condition_model_version >= 1),
 predicates_json TEXT NOT NULL CHECK(json_valid(predicates_json)),
 created_at_ms INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX ux_analysis_index_group_rule_order
 ON analysis_index_group_rule(ruleset_id, display_order);
CREATE UNIQUE INDEX ux_analysis_index_group_rule_group
 ON analysis_index_group_rule(ruleset_id, group_id);
