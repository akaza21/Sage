CREATE TABLE acp_child_thread_transcripts (
  native_child_thread_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  parent_thread_id TEXT NOT NULL,
  snapshot_json TEXT NOT NULL
    CHECK (length(CAST(snapshot_json AS BLOB)) BETWEEN 1 AND 4194304 AND json_valid(snapshot_json)),
  snapshot_bytes INTEGER NOT NULL CHECK (snapshot_bytes BETWEEN 1 AND 4194304),
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    REFERENCES acp_child_thread_owners(project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    ON DELETE RESTRICT,
  CHECK (length(CAST(snapshot_json AS BLOB)) = snapshot_bytes),
  CHECK (native_child_thread_id <> parent_thread_id)
) STRICT;

CREATE INDEX idx_acp_child_thread_transcripts_owner
  ON acp_child_thread_transcripts(project_id, chat_id, session_id, runtime_epoch);
