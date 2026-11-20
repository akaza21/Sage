ALTER TABLE projects ADD COLUMN kind TEXT NOT NULL DEFAULT 'user'
  CHECK (kind IN ('user', 'privateChat'));
CREATE INDEX idx_projects_kind_created ON projects(kind, created_at DESC, id);
