-- Private Codex child-thread identity. Display collaboration labels are not authority.
CREATE TABLE acp_child_thread_owners (
  native_child_thread_id TEXT PRIMARY KEY
    CHECK (length(CAST(native_child_thread_id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  parent_thread_id TEXT NOT NULL
    CHECK (length(CAST(parent_thread_id AS BLOB)) BETWEEN 1 AND 512),
  source TEXT NOT NULL CHECK (source = 'codexSubAgentActivity'),
  created_at INTEGER NOT NULL,
  UNIQUE (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id),
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch)
    REFERENCES session_runtime_epochs(project_id, chat_id, session_id, runtime_epoch) ON DELETE RESTRICT,
  CHECK (native_child_thread_id <> parent_thread_id)
) STRICT;

CREATE TABLE acp_child_thread_bindings (
  item_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  native_child_thread_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, item_id, runtime_epoch)
    REFERENCES items(project_id, chat_id, session_id, turn_id, id, runtime_epoch) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch, native_child_thread_id)
    REFERENCES acp_child_thread_owners(project_id, chat_id, session_id, runtime_epoch, native_child_thread_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX idx_acp_child_thread_bindings_owner
  ON acp_child_thread_bindings(project_id, chat_id, session_id, runtime_epoch, turn_id);
