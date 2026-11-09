-- The native history of a session imported from outside Sage (such as a
-- Codex CLI session), read once at import and shown before the Chat's own
-- Turns. One row per imported session, bounded like a child transcript; it
-- cascades with its session.
CREATE TABLE acp_imported_histories (
  session_id TEXT PRIMARY KEY REFERENCES agent_sessions(id) ON DELETE CASCADE,
  history_json TEXT NOT NULL
    CHECK (json_valid(history_json) AND length(CAST(history_json AS BLOB)) BETWEEN 2 AND 4194304),
  created_at INTEGER NOT NULL
) STRICT;
