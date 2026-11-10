-- Each history page projects a Turn's browser operations and session activity
-- facts. Key those reads by Turn so each one seeks that Turn's facts of one
-- kind, in sequence order, instead of scanning every fact in the session.
CREATE INDEX idx_session_facts_turn
  ON session_facts(session_id, turn_id, kind, sequence);
