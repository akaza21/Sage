CREATE TABLE chat_image_artifacts (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  queue_id TEXT CHECK (queue_id IS NULL OR length(CAST(queue_id AS BLOB)) BETWEEN 1 AND 512),
  session_id TEXT,
  turn_id TEXT,
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('pending', 'queued', 'inFlight', 'retired')),
  mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg')),
  width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 2048),
  height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 2048),
  full_bytes INTEGER NOT NULL CHECK (full_bytes BETWEEN 1 AND 4194304),
  full_sha256 TEXT NOT NULL CHECK (length(full_sha256) = 64),
  thumbnail_bytes INTEGER NOT NULL CHECK (thumbnail_bytes BETWEEN 1 AND 262144),
  thumbnail_sha256 TEXT NOT NULL CHECK (length(thumbnail_sha256) = 64),
  label TEXT NOT NULL CHECK (length(CAST(label AS BLOB)) BETWEEN 1 AND 256),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id) REFERENCES chats(project_id, id) ON DELETE CASCADE,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id)
    REFERENCES turns(project_id, chat_id, session_id, id) ON DELETE RESTRICT,
  CHECK (
    (lifecycle = 'pending' AND queue_id IS NULL AND session_id IS NULL AND turn_id IS NULL) OR
    (lifecycle = 'queued' AND queue_id IS NOT NULL AND session_id IS NULL AND turn_id IS NULL) OR
    (lifecycle IN ('inFlight', 'retired') AND queue_id IS NULL AND session_id IS NOT NULL AND turn_id IS NOT NULL) OR
    (lifecycle = 'retired' AND queue_id IS NULL AND session_id IS NULL AND turn_id IS NULL)
  )
) STRICT;

CREATE INDEX idx_chat_image_artifacts_chat_state
  ON chat_image_artifacts(project_id, chat_id, lifecycle, created_at, id);

CREATE INDEX idx_chat_image_artifacts_queue
  ON chat_image_artifacts(project_id, chat_id, queue_id)
  WHERE queue_id IS NOT NULL;
