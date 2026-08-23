-- Owner-landed schema expansion for durable Ask / Allow-once / Deny.
-- 006 is resource trash. This file is schema version 8 (CurrentV8).

DROP TRIGGER deny_server_request_invalid_transition;
DROP TRIGGER deny_terminal_server_request_mutation;

CREATE TABLE server_requests_v8 (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  binding_digest TEXT NOT NULL
    CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
  external_request_id TEXT NOT NULL
    CHECK (length(CAST(external_request_id AS BLOB)) BETWEEN 1 AND 512),
  kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 128),
  payload_digest_version INTEGER NOT NULL CHECK (payload_digest_version = 1),
  payload_digest TEXT NOT NULL
    CHECK (length(payload_digest) = 64 AND payload_digest NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN (
    'received', 'asked', 'allowPending', 'allowCommitted',
    'declinePending', 'declineCommitted', 'responseQueued',
    'resolvedAllowed', 'resolvedDeclined'
  )),
  normalized_payload_json TEXT NOT NULL CHECK (
    json_valid(normalized_payload_json)
    AND length(CAST(normalized_payload_json AS BLOB)) <= 65536
  ),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  resolved_at INTEGER,
  UNIQUE (binding_digest, session_id, runtime_epoch, external_request_id, kind),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, binding_digest)
    REFERENCES agent_sessions(project_id, chat_id, id, binding_digest) ON DELETE RESTRICT,
  CHECK ((state IN ('resolvedAllowed', 'resolvedDeclined')) = (resolved_at IS NOT NULL))
) STRICT;

INSERT INTO server_requests_v8(
  id, project_id, chat_id, session_id, turn_id, runtime_epoch, binding_digest,
  external_request_id, kind, payload_digest_version, payload_digest, state,
  normalized_payload_json, revision, created_at, updated_at, resolved_at
)
SELECT id, project_id, chat_id, session_id, turn_id, runtime_epoch, binding_digest,
       external_request_id, kind, payload_digest_version, payload_digest, state,
       normalized_payload_json, revision, created_at, updated_at, resolved_at
FROM server_requests;

DROP TABLE server_requests;
ALTER TABLE server_requests_v8 RENAME TO server_requests;

CREATE INDEX idx_server_requests_owner
  ON server_requests(project_id, chat_id, session_id, turn_id, runtime_epoch);

CREATE TRIGGER deny_server_request_invalid_transition
BEFORE UPDATE OF state ON server_requests
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'received' AND NEW.state IN ('asked', 'allowPending', 'declinePending')) OR
  (OLD.state = 'asked' AND NEW.state IN ('allowPending', 'declinePending')) OR
  (OLD.state = 'allowPending' AND NEW.state = 'allowCommitted') OR
  (OLD.state = 'allowCommitted' AND NEW.state = 'responseQueued') OR
  (OLD.state = 'declinePending' AND NEW.state = 'declineCommitted') OR
  (OLD.state = 'declineCommitted' AND NEW.state = 'responseQueued') OR
  (OLD.state = 'responseQueued' AND NEW.state IN ('resolvedAllowed', 'resolvedDeclined'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid server request state transition');
END;

CREATE TRIGGER deny_server_request_identity_mutation
BEFORE UPDATE ON server_requests
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.session_id IS NOT NEW.session_id OR
  OLD.turn_id IS NOT NEW.turn_id OR OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.binding_digest IS NOT NEW.binding_digest OR
  OLD.external_request_id IS NOT NEW.external_request_id OR OLD.kind IS NOT NEW.kind OR
  OLD.payload_digest_version IS NOT NEW.payload_digest_version OR
  OLD.payload_digest IS NOT NEW.payload_digest OR
  OLD.normalized_payload_json IS NOT NEW.normalized_payload_json OR
  OLD.created_at IS NOT NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'server request identity is immutable');
END;

CREATE TRIGGER deny_terminal_server_request_mutation
BEFORE UPDATE ON server_requests
WHEN OLD.state IN ('resolvedAllowed', 'resolvedDeclined') AND (
  OLD.state IS NOT NEW.state OR OLD.revision IS NOT NEW.revision OR
  OLD.updated_at IS NOT NEW.updated_at OR OLD.resolved_at IS NOT NEW.resolved_at
)
BEGIN
  SELECT RAISE(ABORT, 'resolved server request is immutable');
END;

CREATE TRIGGER deny_server_request_delete
BEFORE DELETE ON server_requests
BEGIN
  SELECT RAISE(ABORT, 'server request deletion is not admitted');
END;
