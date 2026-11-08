-- The last session config choice the harness confirmed for each mode or
-- config option of an agent session. A resumed runtime reapplies them, so a
-- choice survives parking and restart. One row per option, rewritten in place
-- (the per-session row count is capped in code); rows cascade with their
-- session.
CREATE TABLE acp_session_config_choices (
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('mode', 'option')),
  config_id TEXT NOT NULL CHECK (length(CAST(config_id AS BLOB)) BETWEEN 1 AND 1024),
  value_json TEXT NOT NULL
    CHECK (json_valid(value_json) AND length(CAST(value_json AS BLOB)) BETWEEN 2 AND 8192),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, kind, config_id)
) STRICT;
