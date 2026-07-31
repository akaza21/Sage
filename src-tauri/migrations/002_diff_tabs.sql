PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE workbench_tabs_v2 (
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

INSERT INTO workbench_tabs_v2 (
  id,
  project_id,
  kind,
  title,
  position,
  pinned,
  dirty,
  status,
  metadata_json,
  created_at,
  updated_at
)
SELECT
  id,
  project_id,
  kind,
  title,
  position,
  pinned,
  dirty,
  status,
  metadata_json,
  created_at,
  updated_at
FROM workbench_tabs
WHERE kind <> 'review';

DROP TABLE workbench_tabs;
ALTER TABLE workbench_tabs_v2 RENAME TO workbench_tabs;

CREATE INDEX idx_tabs_project_position
  ON workbench_tabs(project_id, position);

INSERT INTO schema_migrations(version, applied_at)
VALUES (2, unixepoch('subsec') * 1000);

COMMIT;

PRAGMA foreign_keys = ON;
