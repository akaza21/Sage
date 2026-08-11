CREATE TABLE resource_trash_intents (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  project_generation INTEGER NOT NULL CHECK (project_generation >= 0),
  transition_epoch INTEGER NOT NULL CHECK (transition_epoch >= 0),
  root_id TEXT NOT NULL REFERENCES project_roots(id) ON DELETE RESTRICT,
  source_relative_path TEXT NOT NULL,
  parent_relative_path TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('file', 'emptyFolder')),
  root_identity BLOB NOT NULL CHECK (length(root_identity) = 24),
  parent_identity BLOB NOT NULL CHECK (length(parent_identity) = 24),
  source_identity BLOB NOT NULL CHECK (length(source_identity) = 24),
  identity_digest TEXT NOT NULL CHECK (length(identity_digest) = 64),
  created_at INTEGER NOT NULL
) STRICT;
