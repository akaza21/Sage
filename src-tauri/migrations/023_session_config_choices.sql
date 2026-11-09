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

-- The session config the harness last reported for each agent session: its
-- modes, its options with their labels and choices, and its confirmed model.
-- A parked or restarted chat shows it, with the stored choices above as the
-- current values, until its runtime resumes and reports again. One row per
-- session, rewritten in place; bounded like the live config it copies.
CREATE TABLE acp_session_config_snapshots (
  session_id TEXT PRIMARY KEY REFERENCES agent_sessions(id) ON DELETE CASCADE,
  config_json TEXT NOT NULL
    CHECK (json_valid(config_json) AND length(CAST(config_json AS BLOB)) BETWEEN 2 AND 131072),
  updated_at INTEGER NOT NULL
) STRICT;

-- A stored choice the session could not take back: the option, the saved
-- value, the value the session uses instead, and whether the saved value is
-- no longer offered or was refused. The composer words it. One row per
-- session, replaced, or cleared once that option is chosen again.
CREATE TABLE acp_session_config_notices (
  session_id TEXT PRIMARY KEY REFERENCES agent_sessions(id) ON DELETE CASCADE,
  config_id TEXT NOT NULL CHECK (length(CAST(config_id AS BLOB)) BETWEEN 1 AND 1024),
  saved TEXT NOT NULL CHECK (length(CAST(saved AS BLOB)) BETWEEN 1 AND 1024),
  current TEXT NOT NULL CHECK (length(CAST(current AS BLOB)) BETWEEN 1 AND 1024),
  reason TEXT NOT NULL CHECK (reason IN ('notOffered', 'rejected')),
  updated_at INTEGER NOT NULL
) STRICT;
