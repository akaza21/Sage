-- Each history page projects a Turn's browser operations and session activity
-- facts. Key those reads by Turn so each one seeks that Turn's facts of one
-- kind, in sequence order, instead of scanning every fact in the session.
-- Session-level facts carry no Turn and are never read this way, so they stay
-- out of the index.
CREATE INDEX idx_session_facts_turn
  ON session_facts(session_id, turn_id, kind, sequence)
  WHERE turn_id IS NOT NULL;
