CREATE TABLE chat_queued_prompts (
  project_id TEXT NOT NULL,
  chat_id TEXT PRIMARY KEY,
  prompts_json TEXT NOT NULL CHECK (length(CAST(prompts_json AS BLOB)) <= 524288),
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id) REFERENCES chats(project_id, id) ON DELETE CASCADE
) STRICT;
