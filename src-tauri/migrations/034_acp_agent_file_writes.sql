-- The agent's latest write to each file of a session that rewinds files: one
-- row per session and file, replaced on each write. A null digest is a write
-- whose bytes went unread, so a file rewind cannot verify that file. Rows go
-- with their session (chat purge cascades); turns are never deleted apart.
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

