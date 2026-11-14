-- Capabilities describe one negotiation and refresh atomically on resume.
DROP TRIGGER deny_agent_session_identity_mutation;

CREATE TRIGGER deny_agent_session_identity_mutation
BEFORE UPDATE ON agent_sessions
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.ordinal IS NOT NEW.ordinal OR
  OLD.binding_id IS NOT NEW.binding_id OR OLD.binding_digest IS NOT NEW.binding_digest OR
  OLD.harness_version IS NOT NEW.harness_version OR OLD.model_id IS NOT NEW.model_id OR
  OLD.legacy IS NOT NEW.legacy OR OLD.created_at IS NOT NEW.created_at OR
  (
    (OLD.external_session_id IS NOT NEW.external_session_id OR
     OLD.capabilities_json IS NOT NEW.capabilities_json) AND NOT (
      (
        OLD.state = 'starting' AND NEW.state = 'ready' AND
        OLD.external_session_id IS NULL AND NEW.external_session_id IS NOT NULL AND
        OLD.capabilities_json = '{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}' AND
        NEW.capabilities_json IS NOT OLD.capabilities_json
      ) OR (
        OLD.state = 'recovering' AND NEW.state = 'ready' AND
        OLD.external_session_id IS NEW.external_session_id AND
        json_valid(NEW.capabilities_json)
      )
    )
  )
BEGIN
  SELECT RAISE(ABORT, 'agent session identity is immutable');
END;
