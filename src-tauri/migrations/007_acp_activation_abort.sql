DROP TRIGGER deny_agent_session_invalid_transition;
DROP TRIGGER deny_runtime_epoch_invalid_transition;
DROP TRIGGER deny_terminal_runtime_epoch_mutation;

CREATE TABLE session_runtime_epochs_v7 (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch >= 0),
  state TEXT NOT NULL CHECK (state IN (
    'legacy', 'starting', 'running', 'terminating', 'exited', 'failed', 'aborted'
  )),
  artifact_digest TEXT
    CHECK (artifact_digest IS NULL OR (length(artifact_digest) = 64 AND artifact_digest NOT GLOB '*[^0-9a-f]*')),
  started_at INTEGER NOT NULL,
  terminal_at INTEGER,
  exit_kind TEXT
    CHECK (exit_kind IS NULL OR length(CAST(exit_kind AS BLOB)) BETWEEN 1 AND 128),
  PRIMARY KEY (session_id, runtime_epoch),
  UNIQUE (project_id, chat_id, session_id, runtime_epoch),
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE RESTRICT,
  CHECK ((state IN ('legacy', 'exited', 'failed', 'aborted')) = (terminal_at IS NOT NULL)),
  CHECK (state <> 'aborted' OR exit_kind IS NOT NULL),
  CHECK ((runtime_epoch = 0) = (state = 'legacy'))
) STRICT;

INSERT INTO session_runtime_epochs_v7(
  project_id, chat_id, session_id, runtime_epoch, state, artifact_digest,
  started_at, terminal_at, exit_kind
)
SELECT project_id, chat_id, session_id, runtime_epoch, state, artifact_digest,
       started_at, terminal_at, exit_kind
FROM session_runtime_epochs;

DROP TABLE session_runtime_epochs;
ALTER TABLE session_runtime_epochs_v7 RENAME TO session_runtime_epochs;

CREATE UNIQUE INDEX ux_runtime_epoch_open_per_session
  ON session_runtime_epochs(session_id)
  WHERE state IN ('starting', 'running', 'terminating');

CREATE TRIGGER deny_agent_session_invalid_transition
BEFORE UPDATE OF state ON agent_sessions
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'created' AND NEW.state IN ('starting', 'closing')) OR
  (OLD.state = 'starting' AND NEW.state IN ('ready', 'incompatible', 'failed', 'closing')) OR
  (OLD.state = 'starting' AND NEW.state = 'created' AND
    OLD.residency = 'hot' AND NEW.residency = 'cold' AND
    OLD.external_session_id IS NULL AND NEW.external_session_id IS NULL) OR
  (OLD.state = 'ready' AND NEW.state IN ('active', 'recovering', 'closing', 'failed')) OR
  (OLD.state = 'active' AND NEW.state IN ('ready', 'recovering', 'closing', 'failed')) OR
  (OLD.state = 'recovering' AND NEW.state IN ('ready', 'incompatible', 'failed', 'closing')) OR
  (OLD.state = 'closing' AND NEW.state IN ('closed', 'failed'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid agent session state transition');
END;

CREATE TRIGGER require_agent_session_activation_abort_epoch
BEFORE UPDATE OF state, residency ON agent_sessions
WHEN (
  (OLD.state = 'starting' AND NEW.state = 'created') OR
  (OLD.state = 'recovering' AND NEW.state = 'ready' AND NEW.residency = 'cold')
) AND NOT EXISTS (
  SELECT 1
  FROM session_runtime_epochs e
  WHERE e.project_id = OLD.project_id
    AND e.chat_id = OLD.chat_id
    AND e.session_id = OLD.id
    AND e.state = 'aborted'
    AND e.runtime_epoch = (
      SELECT MAX(latest.runtime_epoch)
      FROM session_runtime_epochs latest
      WHERE latest.session_id = OLD.id
    )
)
BEGIN
  SELECT RAISE(ABORT, 'activation rollback requires its matching aborted runtime epoch');
END;

CREATE TRIGGER deny_runtime_epoch_invalid_transition
BEFORE UPDATE OF state ON session_runtime_epochs
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'starting' AND NEW.state IN ('running', 'terminating', 'failed', 'aborted')) OR
  (OLD.state = 'running' AND NEW.state IN ('terminating', 'exited', 'failed')) OR
  (OLD.state = 'terminating' AND NEW.state IN ('exited', 'failed'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid runtime epoch state transition');
END;

CREATE TRIGGER require_runtime_epoch_activation_abort_owner
BEFORE UPDATE OF state ON session_runtime_epochs
WHEN OLD.state = 'starting' AND NEW.state = 'aborted' AND NOT EXISTS (
  SELECT 1
  FROM agent_sessions s
  WHERE s.project_id = OLD.project_id
    AND s.chat_id = OLD.chat_id
    AND s.id = OLD.session_id
    AND s.residency = 'hot'
    AND (
      (s.state = 'starting' AND s.external_session_id IS NULL) OR
      (s.state = 'recovering' AND s.external_session_id IS NOT NULL)
    )
    AND OLD.runtime_epoch = (
      SELECT MAX(latest.runtime_epoch)
      FROM session_runtime_epochs latest
      WHERE latest.session_id = OLD.session_id
    )
)
BEGIN
  SELECT RAISE(ABORT, 'activation abort requires its latest owning hot session');
END;

CREATE TRIGGER deny_runtime_epoch_identity_mutation
BEFORE UPDATE ON session_runtime_epochs
WHEN OLD.project_id IS NOT NEW.project_id OR OLD.chat_id IS NOT NEW.chat_id OR
  OLD.session_id IS NOT NEW.session_id OR OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.artifact_digest IS NOT NEW.artifact_digest OR OLD.started_at IS NOT NEW.started_at
BEGIN
  SELECT RAISE(ABORT, 'runtime epoch identity is immutable');
END;

CREATE TRIGGER deny_terminal_runtime_epoch_mutation
BEFORE UPDATE ON session_runtime_epochs
WHEN OLD.state IN ('legacy', 'exited', 'failed', 'aborted') AND (
  OLD.state IS NOT NEW.state OR OLD.terminal_at IS NOT NEW.terminal_at OR
  OLD.exit_kind IS NOT NEW.exit_kind
)
BEGIN
  SELECT RAISE(ABORT, 'terminal runtime epoch is immutable');
END;

CREATE TRIGGER deny_runtime_epoch_delete
BEFORE DELETE ON session_runtime_epochs
BEGIN
  SELECT RAISE(ABORT, 'runtime epochs are append-only');
END;
