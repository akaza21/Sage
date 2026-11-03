CREATE TABLE acp_turn_workspace_changes (
  project_id TEXT NOT NULL,
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  client_intent_id TEXT NOT NULL CHECK (length(CAST(client_intent_id AS BLOB)) BETWEEN 1 AND 512),
  root_id TEXT NOT NULL CHECK (length(CAST(root_id AS BLOB)) BETWEEN 1 AND 512),
  root_path TEXT NOT NULL CHECK (length(CAST(root_path AS BLOB)) BETWEEN 1 AND 32768),
  baseline_json TEXT CHECK (baseline_json IS NULL OR length(CAST(baseline_json AS BLOB)) <= 131072),
  result_json TEXT CHECK (result_json IS NULL OR length(CAST(result_json AS BLOB)) <= 131072),
  terminal_json TEXT CHECK (terminal_json IS NULL OR length(CAST(terminal_json AS BLOB)) <= 131072),
  ambiguous INTEGER NOT NULL DEFAULT 0 CHECK (ambiguous IN (0,1)),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes BETWEEN 0 AND 131072),
  PRIMARY KEY (session_id, client_intent_id)
) STRICT;
CREATE INDEX idx_turn_workspace_active ON acp_turn_workspace_changes(root_path) WHERE result_json IS NULL;
CREATE TRIGGER release_turn_workspace_usage AFTER DELETE ON acp_turn_workspace_changes
BEGIN
  UPDATE acp_session_usage SET payload_bytes=payload_bytes-OLD.payload_bytes,revision=revision+1 WHERE session_id=OLD.session_id;
  UPDATE acp_project_usage SET payload_bytes=payload_bytes-OLD.payload_bytes,revision=revision+1
    WHERE project_id=OLD.project_id;
  UPDATE acp_store_usage SET shared_payload_bytes=shared_payload_bytes-OLD.payload_bytes,revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER delete_turn_workspace_changes AFTER DELETE ON turns
BEGIN
  DELETE FROM acp_turn_workspace_changes WHERE session_id=OLD.session_id AND client_intent_id=OLD.client_intent_id;
END;
