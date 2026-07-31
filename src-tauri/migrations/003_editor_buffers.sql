BEGIN IMMEDIATE;

CREATE TABLE editor_buffers (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  base_fingerprint TEXT NOT NULL,
  base_byte_len INTEGER NOT NULL CHECK (base_byte_len >= 0),
  base_modified_at INTEGER,
  line_ending TEXT NOT NULL CHECK (line_ending IN ('lf', 'crlf', 'none')),
  buffer_content TEXT NOT NULL,
  buffer_byte_len INTEGER NOT NULL CHECK (buffer_byte_len >= 0),
  buffer_fingerprint TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision > 0),
  pending_temp_path TEXT,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (project_id, relative_path)
) STRICT;

CREATE INDEX idx_editor_buffers_project_updated
  ON editor_buffers(project_id, updated_at);

INSERT INTO schema_migrations(version, applied_at)
VALUES (3, unixepoch('subsec') * 1000);

COMMIT;
