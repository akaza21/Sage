-- The state each child thread last reported, kept apart from its transcript
-- so the parent projection reads it without parsing the transcript.
CREATE TABLE acp_child_thread_states (
  native_child_thread_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  state TEXT NOT NULL CHECK (state IN ('running', 'completed', 'errored', 'interrupted')),
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    REFERENCES acp_child_thread_owners(project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    ON DELETE RESTRICT
) STRICT;

CREATE INDEX idx_acp_child_thread_states_owner
  ON acp_child_thread_states(project_id, chat_id);
