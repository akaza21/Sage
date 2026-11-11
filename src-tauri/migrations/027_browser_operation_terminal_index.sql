-- A browser operation's terminal fact names its start by sequence. Key
-- terminals by that start so finishing, projecting and restart recovery each
-- seek one operation's terminal instead of reading every terminal in the Turn
-- or session.
CREATE INDEX idx_session_facts_operation_terminal
  ON session_facts(session_id, json_extract(payload_json, '$.s'))
  WHERE kind = 'browserOperationTerminal';
