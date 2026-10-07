CREATE TABLE acp_turn_usage_snapshots (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  input_tokens INTEGER NOT NULL CHECK (input_tokens BETWEEN 0 AND 9223372036854775807),
  output_tokens INTEGER NOT NULL CHECK (output_tokens BETWEEN 0 AND 9223372036854775807),
  context_window INTEGER CHECK (context_window BETWEEN 0 AND 4294967295),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, runtime_epoch, turn_id),
  UNIQUE (project_id, chat_id, session_id, turn_id, runtime_epoch),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE RESTRICT
) STRICT;

CREATE TABLE acp_turn_terminal_reasons (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  kind TEXT NOT NULL CHECK (kind IN ('authRequired','harnessExited','protocolError','cancelledByUser','refusedVersion','timeout','other')),
  message TEXT CHECK (message IS NULL OR length(CAST(message AS BLOB)) BETWEEN 1 AND 8192),
  remediation TEXT CHECK (remediation IS NULL OR length(CAST(remediation AS BLOB)) BETWEEN 1 AND 8192),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, runtime_epoch, turn_id),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE CASCADE
) STRICT;

INSERT INTO acp_turn_usage_snapshots
  (project_id,chat_id,session_id,turn_id,runtime_epoch,input_tokens,output_tokens,context_window,revision,created_at,updated_at)
SELECT project_id,chat_id,session_id,turn_id,runtime_epoch,
       json_extract(payload_json,'$.payload.inputTokens'),
       json_extract(payload_json,'$.payload.outputTokens'),
       json_extract(payload_json,'$.payload.contextWindow'),
       revision,created_at,updated_at
FROM (
  SELECT i.*,
         ROW_NUMBER() OVER (
           PARTITION BY project_id,chat_id,session_id,turn_id,runtime_epoch
           ORDER BY updated_at DESC,revision DESC,id DESC
         ) AS row_number
  FROM items i
  WHERE kind='usage'
)
WHERE row_number=1;

DROP TRIGGER deny_item_delete;
DELETE FROM items WHERE kind='usage';
DROP TABLE IF EXISTS session_events;
CREATE TRIGGER deny_item_delete
BEFORE DELETE ON items
BEGIN
  SELECT RAISE(ABORT, 'item deletion is not admitted in ADE-0006A');
END;
