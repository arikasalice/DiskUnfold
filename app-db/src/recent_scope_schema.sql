CREATE TABLE user_env_recent_scope (
 recent_scope_id BLOB PRIMARY KEY CHECK(length(recent_scope_id) = 16),
 scope_fingerprint BLOB NOT NULL CHECK(length(scope_fingerprint) = 32),
 fingerprint_format INTEGER NOT NULL DEFAULT 1,
 scope_body TEXT NOT NULL CHECK(json_valid(scope_body)),
 include_count INTEGER NOT NULL CHECK(include_count >= 1),
 last_used_at_ms INTEGER NOT NULL,
 created_at_ms INTEGER NOT NULL,
 updated_at_ms INTEGER NOT NULL,
 row_version INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX ux_user_env_recent_scope_fingerprint
 ON user_env_recent_scope(scope_fingerprint, fingerprint_format);
CREATE INDEX ix_user_env_recent_scope_last_used
 ON user_env_recent_scope(last_used_at_ms);
