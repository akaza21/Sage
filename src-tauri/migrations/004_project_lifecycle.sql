CREATE TABLE project_roots_v4 (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  absolute_path TEXT NOT NULL,
  canonical_key TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  UNIQUE (project_id, canonical_key),
  UNIQUE (project_id, ordinal)
) STRICT;

CREATE TABLE editor_buffers_v4 (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  root_id TEXT NOT NULL REFERENCES project_roots_v4(id) ON DELETE CASCADE,
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
  PRIMARY KEY (project_id, root_id, relative_path)
) STRICT;

CREATE TABLE project_layouts (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  active_tab_id TEXT,
  selected_root_id TEXT,
  selected_relative_path TEXT,
  updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE app_state (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  active_project_id TEXT,
  updated_at INTEGER NOT NULL
) STRICT;
