PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_opened_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS project_roots (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  absolute_path TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  PRIMARY KEY (project_id, absolute_path),
  UNIQUE (project_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS workbench_tabs (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (
    kind IN (
      'chat',
      'file',
      'diff',
      'terminal',
      'search',
      'problems',
      'output',
      'settings'
    )
  ),
  title TEXT NOT NULL,
  position INTEGER NOT NULL,
  pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
  dirty INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1)),
  status TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (project_id, position)
) STRICT;

CREATE TABLE IF NOT EXISTS agent_sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  chat_tab_id TEXT NOT NULL REFERENCES workbench_tabs(id) ON DELETE CASCADE,
  harness_id TEXT NOT NULL,
  harness_version TEXT,
  model_id TEXT,
  external_session_id TEXT,
  status TEXT NOT NULL CHECK (
    status IN (
      'idle',
      'working',
      'waiting',
      'recovering',
      'failed',
      'complete',
      'read_only'
    )
  ),
  last_committed_sequence INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS session_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  sequence INTEGER NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (session_id, sequence)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_tabs_project_position
  ON workbench_tabs(project_id, position);

CREATE INDEX IF NOT EXISTS idx_session_events_order
  ON session_events(session_id, sequence);

INSERT OR IGNORE INTO schema_migrations(version, applied_at)
VALUES (1, unixepoch('subsec') * 1000);
