-- Sends seek only this chat's undo starts, including when it has never used
-- Undo. Recovery seeks starts in recovery order across the store. Settlements
-- name their start by sequence, so seek that exact receipt instead of walking
-- a Turn's historical facts. Ordinary facts enter none of these indexes.
CREATE INDEX idx_session_facts_undo_start
  ON session_facts(project_id, chat_id, sequence)
  WHERE kind = 'turnUndoStarted';
CREATE INDEX idx_session_facts_undo_recovery
  ON session_facts(kind, created_at, session_id, sequence)
  WHERE kind = 'turnUndoStarted';
CREATE INDEX idx_session_facts_undo_settled
  ON session_facts(session_id, turn_id, json_extract(payload_json, '$.start'))
  WHERE kind = 'turnUndoSettled';
