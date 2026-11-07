-- One bounded row per agent session: harness title, newest notices, and the
-- live epoch's model and activity rows. Updated in place, never appended.
CREATE TABLE acp_session_surfaces (
  session_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  surface_json TEXT NOT NULL
    CHECK (length(CAST(surface_json AS BLOB)) BETWEEN 2 AND 65536 AND json_valid(surface_json)),
  revision INTEGER NOT NULL CHECK (revision >= 0),
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE CASCADE
) STRICT;

-- The projection revision of the chat's latest send-relevant change. A send
-- built on any projection at or after it is still current. Existing chats
-- start at their projection revision, so they behave exactly as before.
ALTER TABLE chats ADD COLUMN intent_revision INTEGER NOT NULL DEFAULT 0
  CHECK (intent_revision >= 0);
UPDATE chats SET intent_revision = projection_revision;

-- Fail closed: every projection change is send-relevant unless its writer
-- explicitly restores the prior intent revision in the same transaction.
-- Only the session-neutral allowlist does that.
CREATE TRIGGER chats_intent_revision_on_insert
AFTER INSERT ON chats
BEGIN
  UPDATE chats SET intent_revision = NEW.projection_revision
  WHERE project_id = NEW.project_id AND id = NEW.id;
END;

CREATE TRIGGER chats_intent_revision_on_projection
AFTER UPDATE OF projection_revision ON chats
BEGIN
  UPDATE chats SET intent_revision = NEW.projection_revision
  WHERE project_id = NEW.project_id AND id = NEW.id;
END;
