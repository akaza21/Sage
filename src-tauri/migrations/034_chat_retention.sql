-- Explicit archive/restore intent survives closing or reattaching a tab.
ALTER TABLE chats ADD COLUMN retained_at INTEGER;
-- Detached chats in older profiles were explicitly archived. Preserve them
-- before hidden unretained attachments can be released at project boundaries.
UPDATE chats SET retained_at=updated_at
WHERE NOT EXISTS (
  SELECT 1 FROM chat_tabs a WHERE a.project_id=chats.project_id AND a.chat_id=chats.id
);

-- Raw usage counts every retained row. The 64-chat discoverability allowance
-- is enforced by chat creation; raw counts share the global safety ceiling.
-- Rebuild the ledger without changing any counter or release trigger.
CREATE TEMP TABLE chat_retention_usage AS SELECT * FROM acp_project_usage;
DROP TRIGGER release_file_change_snapshot_usage;
DROP TRIGGER release_deleted_item_usage;
DROP TRIGGER release_turn_workspace_usage;
DROP TABLE acp_project_usage;
CREATE TABLE acp_project_usage (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE RESTRICT,
  chat_count INTEGER NOT NULL CHECK (chat_count BETWEEN 0 AND 4096),
  session_count INTEGER NOT NULL CHECK (session_count BETWEEN 0 AND 256),
  turn_count INTEGER NOT NULL CHECK (turn_count BETWEEN 0 AND 8192),
  item_count INTEGER NOT NULL CHECK (item_count >= 0),
  fact_count INTEGER NOT NULL CHECK (fact_count >= 0),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0),
  revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;
INSERT INTO acp_project_usage SELECT * FROM chat_retention_usage;
DROP TABLE chat_retention_usage;

CREATE TRIGGER release_file_change_snapshot_usage
AFTER DELETE ON acp_file_change_snapshots
BEGIN
  UPDATE acp_session_usage
    SET item_bytes=item_bytes-OLD.old_bytes-OLD.new_bytes,
        payload_bytes=payload_bytes-OLD.old_bytes-OLD.new_bytes, revision=revision+1
    WHERE session_id=OLD.session_id;
  UPDATE acp_project_usage
    SET payload_bytes=payload_bytes-OLD.old_bytes-OLD.new_bytes, revision=revision+1
    WHERE project_id=OLD.project_id;
  UPDATE acp_store_usage
    SET item_bytes=item_bytes-OLD.old_bytes-OLD.new_bytes,
        shared_payload_bytes=shared_payload_bytes-OLD.old_bytes-OLD.new_bytes,
        revision=revision+1 WHERE singleton=1;
END;

CREATE TRIGGER release_deleted_item_usage
AFTER DELETE ON items
BEGIN
  UPDATE acp_session_usage
    SET item_count=item_count-1, item_bytes=item_bytes-OLD.payload_bytes,
        payload_bytes=payload_bytes-OLD.payload_bytes, revision=revision+1
    WHERE session_id=OLD.session_id;
  UPDATE acp_project_usage
    SET item_count=item_count-1, payload_bytes=payload_bytes-OLD.payload_bytes,
        revision=revision+1 WHERE project_id=OLD.project_id;
  UPDATE acp_store_usage
    SET item_count=item_count-1, item_bytes=item_bytes-OLD.payload_bytes,
        shared_payload_bytes=shared_payload_bytes-OLD.payload_bytes,
        revision=revision+1 WHERE singleton=1;
END;

CREATE TRIGGER release_turn_workspace_usage AFTER DELETE ON acp_turn_workspace_changes
BEGIN
  UPDATE acp_session_usage SET payload_bytes=payload_bytes-OLD.payload_bytes,revision=revision+1 WHERE session_id=OLD.session_id;
  UPDATE acp_project_usage SET payload_bytes=payload_bytes-OLD.payload_bytes,revision=revision+1
    WHERE project_id=OLD.project_id;
  UPDATE acp_store_usage SET shared_payload_bytes=shared_payload_bytes-OLD.payload_bytes,revision=revision+1 WHERE singleton=1;
END;
