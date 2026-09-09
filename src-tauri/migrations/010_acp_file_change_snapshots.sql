-- Immutable bounded old/new retention for agent file changes (P04).
-- This file is schema version 10 (CurrentV10). It only adds the snapshot
-- tables and accounting triggers; no existing object is rewritten.
-- A summary-only diff has no snapshot row.

-- Keep a high-water mark even after snapshot compaction. Deriving the next
-- revision from retained rows would reuse an already-issued identity.
CREATE TABLE acp_file_change_snapshot_sequences (
  item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
  last_revision INTEGER NOT NULL CHECK (last_revision > 0)
) STRICT;

CREATE TABLE acp_file_change_snapshots (
  item_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  diff_ordinal INTEGER NOT NULL CHECK (diff_ordinal = 0),
  snapshot_revision INTEGER NOT NULL CHECK (snapshot_revision > 0),
  root_id TEXT NOT NULL
    CHECK (length(CAST(root_id AS BLOB)) BETWEEN 1 AND 512),
  relative_path TEXT NOT NULL
    CHECK (length(CAST(relative_path AS BLOB)) BETWEEN 1 AND 16384),
  old_blob BLOB NOT NULL,
  new_blob BLOB NOT NULL,
  old_bytes INTEGER NOT NULL CHECK (old_bytes >= 0 AND old_bytes <= 262144),
  new_bytes INTEGER NOT NULL CHECK (new_bytes >= 0 AND new_bytes <= 262144),
  content_digest TEXT NOT NULL
    CHECK (length(content_digest) = 64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (item_id, snapshot_revision),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, item_id, runtime_epoch)
    REFERENCES items(project_id, chat_id, session_id, turn_id, id, runtime_epoch) ON DELETE CASCADE,
  CHECK (length(old_blob) = old_bytes AND length(new_blob) = new_bytes)
) STRICT;

CREATE INDEX idx_file_change_snapshots_turn
  ON acp_file_change_snapshots(project_id, chat_id, session_id, turn_id, runtime_epoch);

-- Pair rows follow their owned item through ON DELETE CASCADE; only
-- in-place revision mutation is denied below.
CREATE TRIGGER deny_file_change_snapshot_mutation
BEFORE UPDATE ON acp_file_change_snapshots
BEGIN
  SELECT RAISE(ABORT, 'file change snapshot revisions are immutable');
END;

-- Snapshot deletion/compaction and owner cascades release exactly the same
-- shared charge reserved by ingress. Facts remain independently retained.
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
