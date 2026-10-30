CREATE TABLE acp_session_transitions (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  client_intent_id TEXT NOT NULL CHECK(length(CAST(client_intent_id AS BLOB)) BETWEEN 1 AND 512),
  request_digest TEXT NOT NULL CHECK(length(request_digest)=64 AND request_digest NOT GLOB '*[^0-9a-f]*'),
  expected_head TEXT,
  resulting_session TEXT NOT NULL,
  source_watermark INTEGER NOT NULL CHECK(source_watermark>=0),
  projection_revision INTEGER NOT NULL CHECK(projection_revision>=0),
  created_at INTEGER NOT NULL,
  PRIMARY KEY(project_id,chat_id,client_intent_id),
  FOREIGN KEY(project_id,chat_id,expected_head) REFERENCES agent_sessions(project_id,chat_id,id) ON DELETE RESTRICT,
  FOREIGN KEY(project_id,chat_id,resulting_session) REFERENCES agent_sessions(project_id,chat_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE acp_context_handoffs (
  id TEXT PRIMARY KEY CHECK(length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  source_session TEXT,
  target_session TEXT NOT NULL UNIQUE,
  source_watermark INTEGER NOT NULL CHECK(source_watermark>=0),
  plan_json TEXT CHECK(plan_json IS NULL OR (json_valid(plan_json) AND length(CAST(plan_json AS BLOB))<=262144 AND json_array_length(plan_json,'$.selectedIds')<=512)),
  digest TEXT CHECK(digest IS NULL OR (length(digest)=64 AND digest NOT GLOB '*[^0-9a-f]*')),
  state TEXT NOT NULL CHECK(state IN ('prepared','pending','inline','unknown')),
  native_id TEXT CHECK(native_id IS NULL OR length(CAST(native_id AS BLOB)) BETWEEN 1 AND 512),
  runtime_epoch INTEGER CHECK(runtime_epoch IS NULL OR runtime_epoch>0),
  turn_id TEXT,
  projection_revision INTEGER NOT NULL CHECK(projection_revision>=0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(project_id,chat_id,source_session) REFERENCES agent_sessions(project_id,chat_id,id) ON DELETE RESTRICT,
  FOREIGN KEY(project_id,chat_id,target_session) REFERENCES agent_sessions(project_id,chat_id,id) ON DELETE RESTRICT,
  FOREIGN KEY(project_id,chat_id,target_session,turn_id,runtime_epoch) REFERENCES turns(project_id,chat_id,session_id,id,runtime_epoch) ON DELETE RESTRICT,
  CHECK((plan_json IS NULL)=(digest IS NULL)),
  CHECK(state='prepared' OR (plan_json IS NOT NULL AND native_id IS NOT NULL AND runtime_epoch IS NOT NULL AND turn_id IS NOT NULL))
) STRICT;

CREATE INDEX idx_handoff_history ON items(project_id,chat_id);
CREATE INDEX idx_handoff_text_role ON items(project_id,chat_id,kind,
  CASE WHEN json_extract(payload_json,'$.payload.role')='user' THEN 'user' ELSE 'assistant' END);

CREATE TRIGGER immutable_handoff_plan BEFORE UPDATE OF plan_json,digest,source_watermark,source_session,target_session ON acp_context_handoffs
WHEN OLD.plan_json IS NOT NULL OR NEW.source_watermark<>OLD.source_watermark OR NEW.source_session IS NOT OLD.source_session OR NEW.target_session<>OLD.target_session
BEGIN SELECT RAISE(ABORT,'handoff plan is immutable'); END;
