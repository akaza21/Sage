CREATE TABLE acp_child_thread_states (
  native_child_thread_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  turn_id TEXT NOT NULL,
  turn_ordinal INTEGER NOT NULL CHECK (turn_ordinal >= 0),
  state TEXT NOT NULL CHECK (state IN ('running', 'completed', 'errored', 'interrupted')),
  spawn_ended INTEGER NOT NULL CHECK (spawn_ended IN (0, 1)),
  ended_at INTEGER CHECK (ended_at IS NULL OR state <> 'running'),
  ended_turns TEXT NOT NULL CHECK (json_valid(ended_turns) AND json_type(ended_turns) = 'array'),
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    REFERENCES acp_child_thread_owners(project_id, chat_id, session_id, runtime_epoch, native_child_thread_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX idx_acp_child_thread_states_owner ON acp_child_thread_states(project_id, chat_id);

-- A child whose stored transcript already ended keeps that end, recorded
-- with the turn it came from and that turn's place in the child's order. An
-- end from a later turn than the one the child was spawned with is newer than
-- the spawn row. The transcript records no time for it, so none is claimed.
WITH last AS MATERIALIZED (
  SELECT native_child_thread_id, project_id, chat_id, session_id, runtime_epoch,
    json_array_length(snapshot_json, '$.turns') - 1 AS ordinal,
    json_extract(snapshot_json, '$.turns[#-1]') AS turn
  FROM acp_child_thread_transcripts
), ended AS (
  SELECT native_child_thread_id, project_id, chat_id, session_id, runtime_epoch, ordinal,
    json_extract(turn, '$.id') AS turn_id, json_extract(turn, '$.status') AS status
  FROM last
)
INSERT INTO acp_child_thread_states
  (native_child_thread_id, project_id, chat_id, session_id, runtime_epoch, turn_id,
   turn_ordinal, state, spawn_ended, ended_at, ended_turns)
SELECT native_child_thread_id, project_id, chat_id, session_id, runtime_epoch, turn_id,
  ordinal,
  CASE status
    WHEN 'completed' THEN 'completed'
    WHEN 'failed' THEN 'errored'
    ELSE 'interrupted'
  END,
  ordinal > 0, NULL, json_array(turn_id)
FROM ended
WHERE status IN ('completed', 'failed', 'interrupted');
