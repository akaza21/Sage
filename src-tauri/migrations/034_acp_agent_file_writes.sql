-- The agent's latest write to each file of a session that rewinds files: one
-- row per session and file, replaced on each write. A null digest is a write
-- whose bytes went unread, so a file rewind cannot verify that file.
CREATE TABLE acp_agent_file_writes (
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  root_id TEXT NOT NULL CHECK (length(CAST(root_id AS BLOB)) BETWEEN 1 AND 512),
  relative_path TEXT NOT NULL
    CHECK (length(CAST(relative_path AS BLOB)) BETWEEN 1 AND 4096),
  turn_id TEXT NOT NULL CHECK (length(CAST(turn_id AS BLOB)) BETWEEN 1 AND 512),
  digest BLOB CHECK (digest IS NULL OR length(digest) = 32),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, root_id, relative_path)
) STRICT;

-- A write from a Turn that no longer exists proves nothing.
CREATE TRIGGER delete_agent_file_writes AFTER DELETE ON turns
BEGIN
  DELETE FROM acp_agent_file_writes WHERE session_id=OLD.session_id AND turn_id=OLD.id;
END;
