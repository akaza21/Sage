-- Restart recovery pages through browser operation starts and looks up each
-- one's terminal fact, which names its start by sequence. Index the starts on
-- their own so recovery reads only starts, and key terminals by their start so
-- finishing, projecting and recovery each seek one operation's terminal. An
-- operation has at most one terminal, and the unique key holds it to that.
CREATE INDEX idx_session_facts_operation_start
  ON session_facts(session_id, sequence)
  WHERE kind = 'browserOperation';
CREATE UNIQUE INDEX idx_session_facts_operation_terminal
  ON session_facts(session_id, json_extract(payload_json, '$.s'))
  WHERE kind = 'browserOperationTerminal';
