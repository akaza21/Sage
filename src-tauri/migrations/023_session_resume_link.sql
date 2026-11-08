-- A session that continues an earlier session's native thread on a newer
-- harness release names that session here. The link is set once, at insert,
-- on a fresh `created` session; its first activation resumes the linked
-- thread and the ordinary activation fill records the same thread id.
ALTER TABLE agent_sessions ADD COLUMN resumes_session_id TEXT
  CHECK (resumes_session_id IS NULL OR length(CAST(resumes_session_id AS BLOB)) BETWEEN 1 AND 512);

CREATE TRIGGER validate_agent_session_resume_link
BEFORE INSERT ON agent_sessions
WHEN NEW.resumes_session_id IS NOT NULL AND NOT (
  NEW.state = 'created' AND NEW.external_session_id IS NULL AND EXISTS (
    SELECT 1
    FROM agent_sessions earlier
    WHERE earlier.id = NEW.resumes_session_id
      AND earlier.project_id = NEW.project_id
      AND earlier.chat_id = NEW.chat_id
      AND earlier.binding_id = NEW.binding_id
      AND earlier.ordinal < NEW.ordinal
      AND earlier.external_session_id IS NOT NULL
  )
)
BEGIN
  SELECT RAISE(ABORT, 'a resume link must name an earlier native session of the same chat');
END;

CREATE TRIGGER deny_agent_session_resume_link_mutation
BEFORE UPDATE OF resumes_session_id ON agent_sessions
WHEN OLD.resumes_session_id IS NOT NEW.resumes_session_id
BEGIN
  SELECT RAISE(ABORT, 'agent session resume link is immutable');
END;

CREATE TRIGGER require_agent_session_resume_link_thread
BEFORE UPDATE OF external_session_id ON agent_sessions
WHEN NEW.resumes_session_id IS NOT NULL
  AND NEW.external_session_id IS NOT NULL
  AND NEW.external_session_id IS NOT (
    SELECT earlier.external_session_id
    FROM agent_sessions earlier
    WHERE earlier.id = NEW.resumes_session_id
  )
BEGIN
  SELECT RAISE(ABORT, 'a resumed session must fill the linked native thread');
END;
