-- Retries retain their original refusal time; ordinary transcript facts do
-- not enter either command-identity index.
CREATE INDEX idx_session_facts_sign_in_refusal
  ON session_facts(project_id, chat_id, json_extract(payload_json, '$.clientIntentId'))
  WHERE kind = 'commandSignInRefusal';
CREATE INDEX idx_session_facts_session_intent
  ON session_facts(project_id, chat_id, json_extract(payload_json, '$.clientIntentId'))
  WHERE kind IN ('sessionStartIntent', 'sessionActivateIntent');
