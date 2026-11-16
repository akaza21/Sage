CREATE TABLE acp_child_thread_states (
  native_child_thread_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  state TEXT NOT NULL CHECK (state IN ('running', 'completed', 'errored', 'interrupted')),
  turn_id TEXT,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    REFERENCES acp_child_thread_owners(project_id, chat_id, session_id, runtime_epoch, native_child_thread_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX idx_acp_child_thread_states_owner ON acp_child_thread_states(project_id, chat_id);

-- A child whose stored transcript already ended keeps that end.
INSERT INTO acp_child_thread_states
  (native_child_thread_id, project_id, chat_id, session_id, runtime_epoch, state, turn_id, updated_at)
SELECT native_child_thread_id, project_id, chat_id, session_id, runtime_epoch,
  CASE json_extract(snapshot_json, '$.turns[#-1].status')
    WHEN 'completed' THEN 'completed'
    WHEN 'failed' THEN 'errored'
    ELSE 'interrupted'
  END,
  json_extract(snapshot_json, '$.turns[#-1].id'),
  updated_at
FROM acp_child_thread_transcripts
WHERE json_extract(snapshot_json, '$.turns[#-1].status') IN ('completed', 'failed', 'interrupted');
