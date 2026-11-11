-- Restart recovery pages through browser operation starts and looks up each
-- one's terminal fact, which names its start by sequence. Index the starts on
-- their own so recovery reads only starts, and key terminals by their start so
-- finishing, projecting and recovery each seek one operation's terminal.
-- Finishing writes at most one terminal per operation, but the key is not
-- unique: facts are append-only, so a store that already holds a duplicate
-- from an older build could neither drop it nor open behind a unique key.
-- Readers take the earliest terminal, which the trailing sequence key orders.
CREATE INDEX idx_session_facts_operation_start
  ON session_facts(session_id, sequence)
  WHERE kind = 'browserOperation';
CREATE INDEX idx_session_facts_operation_terminal
  ON session_facts(session_id, json_extract(payload_json, '$.s'), sequence)
  WHERE kind = 'browserOperationTerminal';
