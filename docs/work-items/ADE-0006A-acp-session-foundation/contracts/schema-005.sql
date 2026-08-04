-- ADE-0006A C0 final-schema contract for migration 005.
-- Specification only: this file intentionally excludes legacy table renames,
-- bounded conversion, backup publication, and schema_migrations/user_version
-- updates. The production migration must produce this schema atomically.
-- Prerequisites supplied by migrations 001-004: projects and workbench_tabs.

CREATE UNIQUE INDEX IF NOT EXISTS uq_workbench_tabs_project_id
  ON workbench_tabs(project_id, id);

CREATE TABLE chats (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  title TEXT NOT NULL CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 1024),
  projection_revision INTEGER NOT NULL DEFAULT 0 CHECK (projection_revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (project_id, ordinal),
  UNIQUE (project_id, id)
) STRICT;

CREATE TABLE chat_tabs (
  project_id TEXT NOT NULL,
  chat_id TEXT PRIMARY KEY,
  tab_id TEXT NOT NULL UNIQUE,
  attached_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id)
    REFERENCES chats(project_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, tab_id)
    REFERENCES workbench_tabs(project_id, id) ON DELETE CASCADE
) STRICT;

CREATE INDEX idx_chat_tabs_project
  ON chat_tabs(project_id, chat_id);

CREATE TABLE agent_sessions (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  binding_id TEXT NOT NULL
    CHECK (length(CAST(binding_id AS BLOB)) BETWEEN 1 AND 512),
  binding_digest TEXT NOT NULL
    CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
  harness_version TEXT
    CHECK (harness_version IS NULL OR length(CAST(harness_version AS BLOB)) BETWEEN 1 AND 512),
  model_id TEXT
    CHECK (model_id IS NULL OR length(CAST(model_id AS BLOB)) BETWEEN 1 AND 512),
  external_session_id TEXT
    CHECK (external_session_id IS NULL OR length(CAST(external_session_id AS BLOB)) BETWEEN 1 AND 512),
  capabilities_json TEXT NOT NULL
    CHECK (json_valid(capabilities_json) AND length(CAST(capabilities_json AS BLOB)) <= 65536),
  state TEXT NOT NULL CHECK (state IN (
    'created', 'starting', 'ready', 'active', 'recovering',
    'closing', 'closed', 'failed', 'incompatible'
  )),
  residency TEXT NOT NULL CHECK (residency IN ('hot', 'cold')),
  legacy INTEGER NOT NULL DEFAULT 0 CHECK (legacy IN (0, 1)),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  projection_revision INTEGER NOT NULL DEFAULT 0 CHECK (projection_revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  terminal_at INTEGER,
  FOREIGN KEY (project_id, chat_id)
    REFERENCES chats(project_id, id) ON DELETE RESTRICT,
  UNIQUE (chat_id, ordinal),
  UNIQUE (project_id, chat_id, id),
  UNIQUE (project_id, chat_id, id, binding_digest),
  CHECK (legacy = 0 OR (state = 'closed' AND residency = 'cold')),
  CHECK (residency <> 'hot' OR state IN ('starting', 'ready', 'active', 'recovering', 'closing')),
  CHECK ((state IN ('closed', 'failed', 'incompatible')) = (terminal_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_agent_sessions_owner
  ON agent_sessions(project_id, chat_id, ordinal, id);

CREATE UNIQUE INDEX ux_agent_session_nonterminal_per_chat
  ON agent_sessions(chat_id)
  WHERE state IN ('created', 'starting', 'ready', 'active', 'recovering', 'closing');

CREATE TABLE chat_session_heads (
  project_id TEXT NOT NULL,
  chat_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL UNIQUE,
  installed_revision INTEGER NOT NULL CHECK (installed_revision >= 0),
  installed_at INTEGER NOT NULL,
  FOREIGN KEY (project_id, chat_id)
    REFERENCES chats(project_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE session_runtime_epochs (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch >= 0),
  state TEXT NOT NULL CHECK (state IN ('legacy', 'starting', 'running', 'terminating', 'exited', 'failed')),
  artifact_digest TEXT
    CHECK (artifact_digest IS NULL OR (length(artifact_digest) = 64 AND artifact_digest NOT GLOB '*[^0-9a-f]*')),
  started_at INTEGER NOT NULL,
  terminal_at INTEGER,
  exit_kind TEXT
    CHECK (exit_kind IS NULL OR length(CAST(exit_kind AS BLOB)) BETWEEN 1 AND 128),
  PRIMARY KEY (session_id, runtime_epoch),
  UNIQUE (project_id, chat_id, session_id, runtime_epoch),
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE RESTRICT,
  CHECK ((state IN ('legacy', 'exited', 'failed')) = (terminal_at IS NOT NULL)),
  CHECK ((runtime_epoch = 0) = (state = 'legacy'))
) STRICT;

CREATE UNIQUE INDEX ux_runtime_epoch_open_per_session
  ON session_runtime_epochs(session_id)
  WHERE state IN ('starting', 'running', 'terminating');

CREATE TABLE turns (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch >= 0),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  client_intent_id TEXT NOT NULL
    CHECK (length(CAST(client_intent_id AS BLOB)) BETWEEN 1 AND 512),
  state TEXT NOT NULL CHECK (state IN (
    'queued', 'sending', 'running', 'waitingModel', 'waitingTool',
    'waitingApproval', 'waitingUser', 'cancelling', 'completed',
    'interrupted', 'cancelled', 'failed', 'deliveryUnknown'
  )),
  delivery_state TEXT NOT NULL CHECK (delivery_state IN (
    'notWritten', 'writing', 'zeroBytes', 'possibleBytes', 'completeBytes', 'correlated'
  )),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  terminal_at INTEGER,
  FOREIGN KEY (project_id, chat_id, session_id, runtime_epoch)
    REFERENCES session_runtime_epochs(project_id, chat_id, session_id, runtime_epoch) ON DELETE RESTRICT,
  UNIQUE (session_id, ordinal),
  UNIQUE (session_id, client_intent_id),
  UNIQUE (project_id, chat_id, session_id, id),
  UNIQUE (project_id, chat_id, session_id, id, runtime_epoch),
  CHECK ((state IN ('completed', 'interrupted', 'cancelled', 'failed', 'deliveryUnknown')) = (terminal_at IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX ux_turn_nonterminal_per_session
  ON turns(session_id)
  WHERE state IN (
    'queued', 'sending', 'running', 'waitingModel', 'waitingTool',
    'waitingApproval', 'waitingUser', 'cancelling'
  );

CREATE INDEX idx_turns_history
  ON turns(session_id, ordinal DESC, id DESC);

CREATE TABLE session_activities (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch >= 0),
  external_activity_id TEXT NOT NULL
    CHECK (length(CAST(external_activity_id AS BLOB)) BETWEEN 1 AND 512),
  activity_kind TEXT NOT NULL
    CHECK (length(CAST(activity_kind AS BLOB)) BETWEEN 1 AND 128),
  parent_activity_id TEXT,
  parent_item_id TEXT NOT NULL
    CHECK (length(CAST(parent_item_id AS BLOB)) BETWEEN 1 AND 512),
  role TEXT NOT NULL CHECK (role IN ('root', 'fork', 'child', 'unknown')),
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('observed', 'active', 'completed', 'failed', 'cancelled', 'unsupported')),
  depth INTEGER NOT NULL CHECK (depth BETWEEN 0 AND 4),
  observed_sequence INTEGER NOT NULL CHECK (observed_sequence > 0),
  provenance_json TEXT NOT NULL
    CHECK (json_valid(provenance_json) AND length(CAST(provenance_json AS BLOB)) <= 65536),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  terminal_at INTEGER,
  UNIQUE (session_id, runtime_epoch, activity_kind, external_activity_id),
  UNIQUE (project_id, chat_id, session_id, turn_id, runtime_epoch, id),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch, parent_activity_id)
    REFERENCES session_activities(project_id, chat_id, session_id, turn_id, runtime_epoch, id) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, parent_item_id, runtime_epoch)
    REFERENCES items(project_id, chat_id, session_id, turn_id, id, runtime_epoch) ON DELETE RESTRICT,
  CHECK ((lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported')) = (terminal_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_session_activities_parent
  ON session_activities(project_id, chat_id, session_id, turn_id, runtime_epoch, parent_activity_id);

CREATE TABLE items (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch >= 0),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  origin TEXT NOT NULL CHECK (origin IN ('primary', 'harnessActivity')),
  activity_id TEXT,
  kind TEXT NOT NULL CHECK (kind IN (
    'text', 'thoughtSummary', 'plan', 'toolCall', 'toolUpdateResult',
    'fileChangeDiff', 'usage', 'permissionRequest', 'error', 'unsupported',
    'collaboration'
  )),
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('started', 'streaming', 'completed', 'failed', 'cancelled', 'unsupported')),
  tool_state TEXT CHECK (tool_state IS NULL OR tool_state IN (
    'pending', 'inProgress', 'waitingForApproval', 'completed', 'failed', 'declined', 'cancelled'
  )),
  external_item_id TEXT
    CHECK (external_item_id IS NULL OR length(CAST(external_item_id AS BLOB)) BETWEEN 1 AND 512),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0 AND payload_bytes <= 262144),
  payload_digest TEXT NOT NULL
    CHECK (length(payload_digest) = 64 AND payload_digest NOT GLOB '*[^0-9a-f]*'),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  terminal_at INTEGER,
  UNIQUE (turn_id, ordinal),
  UNIQUE (project_id, chat_id, session_id, turn_id, id, runtime_epoch),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch, activity_id)
    REFERENCES session_activities(project_id, chat_id, session_id, turn_id, runtime_epoch, id) ON DELETE RESTRICT,
  CHECK (payload_bytes = length(CAST(payload_json AS BLOB))),
  CHECK ((origin = 'primary' AND activity_id IS NULL) OR (origin = 'harnessActivity' AND activity_id IS NOT NULL)),
  CHECK ((kind IN ('toolCall', 'toolUpdateResult', 'permissionRequest')) = (tool_state IS NOT NULL)),
  CHECK (tool_state IS NULL OR ((lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported')) = (tool_state IN ('completed', 'failed', 'declined', 'cancelled')))),
  CHECK ((lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported')) = (terminal_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_items_owner
  ON items(project_id, chat_id, session_id, turn_id, ordinal);

CREATE UNIQUE INDEX ux_items_external_epoch_kind
  ON items(session_id, runtime_epoch, kind, external_item_id)
  WHERE external_item_id IS NOT NULL;

CREATE TABLE chat_lineage (
  child_project_id TEXT NOT NULL,
  child_chat_id TEXT PRIMARY KEY,
  parent_project_id TEXT NOT NULL,
  parent_chat_id TEXT NOT NULL,
  source_session_id TEXT NOT NULL,
  source_turn_id TEXT NOT NULL,
  relation TEXT NOT NULL CHECK (relation = 'nativeFork'),
  external_operation_id TEXT
    CHECK (external_operation_id IS NULL OR length(CAST(external_operation_id AS BLOB)) BETWEEN 1 AND 512),
  created_at INTEGER NOT NULL,
  FOREIGN KEY (child_project_id, child_chat_id)
    REFERENCES chats(project_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (parent_project_id, parent_chat_id)
    REFERENCES chats(project_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (parent_project_id, parent_chat_id, source_session_id, source_turn_id)
    REFERENCES turns(project_id, chat_id, session_id, id) ON DELETE RESTRICT,
  CHECK (child_chat_id <> parent_chat_id),
  CHECK (child_project_id = parent_project_id)
) STRICT;

CREATE INDEX idx_chat_lineage_parent
  ON chat_lineage(parent_project_id, parent_chat_id, child_chat_id);

CREATE TABLE server_requests (
  id TEXT PRIMARY KEY
    CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 512),
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  runtime_epoch INTEGER NOT NULL CHECK (runtime_epoch > 0),
  binding_digest TEXT NOT NULL
    CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
  external_request_id TEXT NOT NULL
    CHECK (length(CAST(external_request_id AS BLOB)) BETWEEN 1 AND 512),
  kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 128),
  payload_digest_version INTEGER NOT NULL CHECK (payload_digest_version = 1),
  payload_digest TEXT NOT NULL
    CHECK (length(payload_digest) = 64 AND payload_digest NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN (
    'received', 'declinePending', 'declineCommitted', 'responseQueued', 'resolvedDeclined'
  )),
  normalized_payload_json TEXT NOT NULL CHECK (
    json_valid(normalized_payload_json)
    AND length(CAST(normalized_payload_json AS BLOB)) <= 65536
  ),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  resolved_at INTEGER,
  UNIQUE (binding_digest, session_id, runtime_epoch, external_request_id, kind),
  FOREIGN KEY (project_id, chat_id, session_id, turn_id, runtime_epoch)
    REFERENCES turns(project_id, chat_id, session_id, id, runtime_epoch) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, binding_digest)
    REFERENCES agent_sessions(project_id, chat_id, id, binding_digest) ON DELETE RESTRICT,
  CHECK ((state = 'resolvedDeclined') = (resolved_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_server_requests_owner
  ON server_requests(project_id, chat_id, session_id, turn_id, runtime_epoch);

CREATE TABLE session_facts (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  sequence INTEGER NOT NULL CHECK (sequence > 0),
  turn_id TEXT,
  kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 128),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0 AND payload_bytes <= 8192),
  payload_digest TEXT NOT NULL
    CHECK (length(payload_digest) = 64 AND payload_digest NOT GLOB '*[^0-9a-f]*'),
  projection_revision INTEGER NOT NULL CHECK (projection_revision > 0),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, sequence),
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (project_id, chat_id, session_id, turn_id)
    REFERENCES turns(project_id, chat_id, session_id, id) ON DELETE RESTRICT,
  CHECK (payload_bytes = length(CAST(payload_json AS BLOB)))
) STRICT;

CREATE INDEX idx_session_facts_owner
  ON session_facts(project_id, chat_id, session_id, sequence);

CREATE TABLE acp_project_usage (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE RESTRICT,
  chat_count INTEGER NOT NULL CHECK (chat_count BETWEEN 0 AND 64),
  session_count INTEGER NOT NULL CHECK (session_count BETWEEN 0 AND 256),
  turn_count INTEGER NOT NULL CHECK (turn_count BETWEEN 0 AND 8192),
  item_count INTEGER NOT NULL CHECK (item_count >= 0),
  fact_count INTEGER NOT NULL CHECK (fact_count >= 0),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0),
  revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;

CREATE TABLE acp_session_usage (
  project_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  session_id TEXT PRIMARY KEY,
  turn_count INTEGER NOT NULL CHECK (turn_count BETWEEN 0 AND 5000),
  item_count INTEGER NOT NULL CHECK (item_count BETWEEN 0 AND 5000),
  item_bytes INTEGER NOT NULL CHECK (item_bytes BETWEEN 0 AND 67108864),
  fact_count INTEGER NOT NULL CHECK (fact_count BETWEEN 0 AND 20000),
  fact_bytes INTEGER NOT NULL CHECK (fact_bytes BETWEEN 0 AND 67108864),
  payload_bytes INTEGER NOT NULL CHECK (payload_bytes BETWEEN 0 AND 268435456),
  revision INTEGER NOT NULL CHECK (revision >= 0),
  FOREIGN KEY (project_id, chat_id, session_id)
    REFERENCES agent_sessions(project_id, chat_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE acp_store_usage (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  chat_count INTEGER NOT NULL CHECK (chat_count BETWEEN 0 AND 4096),
  session_count INTEGER NOT NULL CHECK (session_count BETWEEN 0 AND 4096),
  turn_count INTEGER NOT NULL CHECK (turn_count BETWEEN 0 AND 32768),
  item_count INTEGER NOT NULL CHECK (item_count BETWEEN 0 AND 50000),
  item_bytes INTEGER NOT NULL CHECK (item_bytes BETWEEN 0 AND 268435456),
  fact_count INTEGER NOT NULL CHECK (fact_count BETWEEN 0 AND 200000),
  fact_bytes INTEGER NOT NULL CHECK (fact_bytes BETWEEN 0 AND 67108864),
  shared_payload_bytes INTEGER NOT NULL CHECK (shared_payload_bytes BETWEEN 0 AND 536870912),
  revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;

CREATE TRIGGER deny_agent_session_invalid_transition
BEFORE UPDATE OF state ON agent_sessions
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'created' AND NEW.state IN ('starting', 'closing')) OR
  (OLD.state = 'starting' AND NEW.state IN ('ready', 'incompatible', 'failed', 'closing')) OR
  (OLD.state = 'ready' AND NEW.state IN ('active', 'recovering', 'closing', 'failed')) OR
  (OLD.state = 'active' AND NEW.state IN ('ready', 'recovering', 'closing', 'failed')) OR
  (OLD.state = 'recovering' AND NEW.state IN ('ready', 'incompatible', 'failed', 'closing')) OR
  (OLD.state = 'closing' AND NEW.state IN ('closed', 'failed'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid agent session state transition');
END;

CREATE TRIGGER deny_agent_session_identity_mutation
BEFORE UPDATE ON agent_sessions
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.ordinal IS NOT NEW.ordinal OR
  OLD.binding_id IS NOT NEW.binding_id OR OLD.binding_digest IS NOT NEW.binding_digest OR
  OLD.harness_version IS NOT NEW.harness_version OR OLD.model_id IS NOT NEW.model_id OR
  OLD.legacy IS NOT NEW.legacy OR OLD.created_at IS NOT NEW.created_at OR
  (
    (OLD.external_session_id IS NOT NEW.external_session_id OR
     OLD.capabilities_json IS NOT NEW.capabilities_json) AND NOT (
      OLD.state = 'starting' AND NEW.state = 'ready' AND
      OLD.external_session_id IS NULL AND NEW.external_session_id IS NOT NULL AND
      OLD.capabilities_json = '{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}' AND
      NEW.capabilities_json IS NOT OLD.capabilities_json
    )
  )
BEGIN
  SELECT RAISE(ABORT, 'agent session identity is immutable');
END;

CREATE TRIGGER require_agent_session_activation_identity_fill
BEFORE UPDATE OF state ON agent_sessions
WHEN OLD.state = 'starting' AND NEW.state = 'ready' AND (
  NEW.external_session_id IS NULL OR
  OLD.capabilities_json <> '{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}' OR
  NEW.capabilities_json IS OLD.capabilities_json
)
BEGIN
  SELECT RAISE(ABORT, 'starting session must atomically fill activation identity');
END;

CREATE TRIGGER deny_terminal_agent_session_mutation
BEFORE UPDATE ON agent_sessions
WHEN OLD.state IN ('closed', 'failed', 'incompatible') AND (
  OLD.state IS NOT NEW.state OR OLD.residency IS NOT NEW.residency OR
  OLD.revision IS NOT NEW.revision OR OLD.projection_revision IS NOT NEW.projection_revision OR
  OLD.updated_at IS NOT NEW.updated_at OR OLD.terminal_at IS NOT NEW.terminal_at
)
BEGIN
  SELECT RAISE(ABORT, 'terminal agent session is immutable');
END;

CREATE TRIGGER validate_chat_session_head_insert
BEFORE INSERT ON chat_session_heads
WHEN NOT EXISTS (
  SELECT 1
  FROM agent_sessions s
  WHERE s.project_id = NEW.project_id
    AND s.chat_id = NEW.chat_id
    AND s.id = NEW.session_id
    AND s.state IN ('created', 'starting', 'ready', 'active', 'recovering', 'closing')
)
BEGIN
  SELECT RAISE(ABORT, 'chat head must reference a nonterminal owning session');
END;

CREATE TRIGGER deny_chat_session_head_update
BEFORE UPDATE ON chat_session_heads
BEGIN
  SELECT RAISE(ABORT, 'chat head replacement is delete-then-insert in one transaction');
END;

CREATE TRIGGER deny_terminal_session_with_head
BEFORE UPDATE OF state ON agent_sessions
WHEN NEW.state IN ('closed', 'failed', 'incompatible')
  AND EXISTS (SELECT 1 FROM chat_session_heads h WHERE h.session_id = OLD.id)
BEGIN
  SELECT RAISE(ABORT, 'remove chat head before terminal session transition');
END;

CREATE TRIGGER deny_runtime_epoch_invalid_transition
BEFORE UPDATE OF state ON session_runtime_epochs
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'starting' AND NEW.state IN ('running', 'terminating', 'failed')) OR
  (OLD.state = 'running' AND NEW.state IN ('terminating', 'exited', 'failed')) OR
  (OLD.state = 'terminating' AND NEW.state IN ('exited', 'failed'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid runtime epoch state transition');
END;

CREATE TRIGGER deny_runtime_epoch_identity_mutation
BEFORE UPDATE ON session_runtime_epochs
WHEN OLD.project_id IS NOT NEW.project_id OR OLD.chat_id IS NOT NEW.chat_id OR
  OLD.session_id IS NOT NEW.session_id OR OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.artifact_digest IS NOT NEW.artifact_digest OR OLD.started_at IS NOT NEW.started_at
BEGIN
  SELECT RAISE(ABORT, 'runtime epoch identity is immutable');
END;

CREATE TRIGGER deny_terminal_runtime_epoch_mutation
BEFORE UPDATE ON session_runtime_epochs
WHEN OLD.state IN ('legacy', 'exited', 'failed') AND (
  OLD.state IS NOT NEW.state OR OLD.terminal_at IS NOT NEW.terminal_at OR
  OLD.exit_kind IS NOT NEW.exit_kind
)
BEGIN
  SELECT RAISE(ABORT, 'terminal runtime epoch is immutable');
END;

CREATE TRIGGER deny_turn_invalid_transition
BEFORE UPDATE OF state ON turns
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'queued' AND NEW.state IN ('sending', 'cancelled')) OR
  (OLD.state = 'sending' AND NEW.state IN ('running', 'cancelling', 'cancelled', 'failed', 'deliveryUnknown')) OR
  (OLD.state = 'running'
    AND NEW.state IN ('waitingModel', 'waitingTool', 'waitingApproval', 'waitingUser', 'cancelling', 'completed', 'interrupted', 'cancelled', 'failed')) OR
  (OLD.state IN ('waitingModel', 'waitingTool', 'waitingApproval', 'waitingUser')
    AND NEW.state IN ('running', 'cancelling', 'completed', 'interrupted', 'cancelled', 'failed')) OR
  (OLD.state = 'cancelling' AND NEW.state IN ('cancelled', 'interrupted', 'failed', 'deliveryUnknown'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid turn state transition');
END;

CREATE TRIGGER deny_turn_identity_mutation
BEFORE UPDATE ON turns
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.session_id IS NOT NEW.session_id OR
  OLD.runtime_epoch IS NOT NEW.runtime_epoch OR OLD.ordinal IS NOT NEW.ordinal OR
  OLD.client_intent_id IS NOT NEW.client_intent_id OR OLD.created_at IS NOT NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'turn identity is immutable');
END;

CREATE TRIGGER deny_turn_delivery_regression
BEFORE UPDATE OF delivery_state ON turns
WHEN NOT (
  OLD.delivery_state = NEW.delivery_state OR
  (OLD.delivery_state = 'notWritten' AND NEW.delivery_state IN ('writing', 'zeroBytes')) OR
  (OLD.delivery_state = 'zeroBytes' AND NEW.delivery_state = 'writing') OR
  (OLD.delivery_state = 'writing' AND NEW.delivery_state IN ('zeroBytes', 'possibleBytes', 'completeBytes')) OR
  (OLD.delivery_state IN ('possibleBytes', 'completeBytes') AND NEW.delivery_state = 'correlated')
)
BEGIN
  SELECT RAISE(ABORT, 'invalid turn delivery transition');
END;

CREATE TRIGGER deny_terminal_turn_mutation
BEFORE UPDATE ON turns
WHEN OLD.state IN ('completed', 'interrupted', 'cancelled', 'failed', 'deliveryUnknown') AND (
  OLD.state IS NOT NEW.state OR OLD.delivery_state IS NOT NEW.delivery_state OR
  OLD.revision IS NOT NEW.revision OR OLD.updated_at IS NOT NEW.updated_at OR
  OLD.terminal_at IS NOT NEW.terminal_at
)
BEGIN
  SELECT RAISE(ABORT, 'terminal turn is immutable');
END;

CREATE TRIGGER deny_item_invalid_transition
BEFORE UPDATE OF lifecycle ON items
WHEN NOT (
  OLD.lifecycle = NEW.lifecycle OR
  (OLD.lifecycle = 'started' AND NEW.lifecycle IN ('streaming', 'completed', 'failed', 'cancelled', 'unsupported')) OR
  (OLD.lifecycle = 'streaming' AND NEW.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid item lifecycle transition');
END;

CREATE TRIGGER deny_terminal_item_mutation
BEFORE UPDATE ON items
WHEN OLD.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported') AND (
  OLD.project_id IS NOT NEW.project_id OR OLD.chat_id IS NOT NEW.chat_id OR
  OLD.session_id IS NOT NEW.session_id OR OLD.turn_id IS NOT NEW.turn_id OR
  OLD.runtime_epoch IS NOT NEW.runtime_epoch OR OLD.ordinal IS NOT NEW.ordinal OR
  OLD.origin IS NOT NEW.origin OR OLD.activity_id IS NOT NEW.activity_id OR
  OLD.kind IS NOT NEW.kind OR OLD.lifecycle IS NOT NEW.lifecycle OR
  OLD.tool_state IS NOT NEW.tool_state OR OLD.external_item_id IS NOT NEW.external_item_id OR
  OLD.payload_json IS NOT NEW.payload_json OR OLD.payload_bytes IS NOT NEW.payload_bytes OR
  OLD.payload_digest IS NOT NEW.payload_digest OR OLD.revision IS NOT NEW.revision OR
  OLD.created_at IS NOT NEW.created_at OR OLD.updated_at IS NOT NEW.updated_at OR
  OLD.terminal_at IS NOT NEW.terminal_at
)
BEGIN
  SELECT RAISE(ABORT, 'terminal item is immutable; identical duplicate is a no-op');
END;

CREATE TRIGGER deny_activity_invalid_transition
BEFORE UPDATE OF lifecycle ON session_activities
WHEN NOT (
  OLD.lifecycle = NEW.lifecycle OR
  (OLD.lifecycle = 'observed' AND NEW.lifecycle IN ('active', 'completed', 'failed', 'cancelled', 'unsupported')) OR
  (OLD.lifecycle = 'active' AND NEW.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid harness activity lifecycle transition');
END;

CREATE TRIGGER deny_activity_identity_mutation
BEFORE UPDATE ON session_activities
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.session_id IS NOT NEW.session_id OR
  OLD.turn_id IS NOT NEW.turn_id OR OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.external_activity_id IS NOT NEW.external_activity_id OR
  OLD.activity_kind IS NOT NEW.activity_kind OR
  OLD.parent_activity_id IS NOT NEW.parent_activity_id OR
  OLD.parent_item_id IS NOT NEW.parent_item_id OR OLD.role IS NOT NEW.role OR
  OLD.depth IS NOT NEW.depth OR OLD.observed_sequence IS NOT NEW.observed_sequence OR
  OLD.provenance_json IS NOT NEW.provenance_json OR OLD.created_at IS NOT NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'harness activity identity is immutable');
END;

CREATE TRIGGER deny_terminal_activity_mutation
BEFORE UPDATE ON session_activities
WHEN OLD.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported') AND (
  OLD.project_id IS NOT NEW.project_id OR OLD.chat_id IS NOT NEW.chat_id OR
  OLD.session_id IS NOT NEW.session_id OR OLD.turn_id IS NOT NEW.turn_id OR
  OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.external_activity_id IS NOT NEW.external_activity_id OR
  OLD.activity_kind IS NOT NEW.activity_kind OR
  OLD.parent_activity_id IS NOT NEW.parent_activity_id OR
  OLD.parent_item_id IS NOT NEW.parent_item_id OR OLD.role IS NOT NEW.role OR
  OLD.lifecycle IS NOT NEW.lifecycle OR OLD.depth IS NOT NEW.depth OR
  OLD.observed_sequence IS NOT NEW.observed_sequence OR
  OLD.provenance_json IS NOT NEW.provenance_json OR OLD.revision IS NOT NEW.revision OR
  OLD.created_at IS NOT NEW.created_at OR OLD.updated_at IS NOT NEW.updated_at OR
  OLD.terminal_at IS NOT NEW.terminal_at
)
BEGIN
  SELECT RAISE(ABORT, 'terminal harness activity is immutable');
END;

CREATE TRIGGER deny_tool_invalid_transition
BEFORE UPDATE OF tool_state ON items
WHEN OLD.tool_state IS NOT NULL AND NOT (
  OLD.tool_state = NEW.tool_state OR
  (OLD.tool_state = 'pending' AND NEW.tool_state IN ('inProgress', 'waitingForApproval', 'completed', 'failed', 'declined', 'cancelled')) OR
  (OLD.tool_state = 'inProgress' AND NEW.tool_state IN ('waitingForApproval', 'completed', 'failed', 'declined', 'cancelled')) OR
  (OLD.tool_state = 'waitingForApproval' AND NEW.tool_state IN ('inProgress', 'completed', 'failed', 'declined', 'cancelled'))
)
BEGIN
  SELECT RAISE(ABORT, 'invalid tool state transition');
END;

CREATE TRIGGER deny_server_request_invalid_transition
BEFORE UPDATE OF state ON server_requests
WHEN NOT (
  OLD.state = NEW.state OR
  (OLD.state = 'received' AND NEW.state = 'declinePending') OR
  (OLD.state = 'declinePending' AND NEW.state = 'declineCommitted') OR
  (OLD.state = 'declineCommitted' AND NEW.state = 'responseQueued') OR
  (OLD.state = 'responseQueued' AND NEW.state = 'resolvedDeclined')
)
BEGIN
  SELECT RAISE(ABORT, 'invalid server request state transition');
END;

CREATE TRIGGER deny_server_request_identity_mutation
BEFORE UPDATE ON server_requests
WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR
  OLD.chat_id IS NOT NEW.chat_id OR OLD.session_id IS NOT NEW.session_id OR
  OLD.turn_id IS NOT NEW.turn_id OR OLD.runtime_epoch IS NOT NEW.runtime_epoch OR
  OLD.binding_digest IS NOT NEW.binding_digest OR
  OLD.external_request_id IS NOT NEW.external_request_id OR OLD.kind IS NOT NEW.kind OR
  OLD.payload_digest_version IS NOT NEW.payload_digest_version OR
  OLD.payload_digest IS NOT NEW.payload_digest OR
  OLD.normalized_payload_json IS NOT NEW.normalized_payload_json OR
  OLD.created_at IS NOT NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'server request identity is immutable');
END;

CREATE TRIGGER deny_terminal_server_request_mutation
BEFORE UPDATE ON server_requests
WHEN OLD.state = 'resolvedDeclined' AND (
  OLD.state IS NOT NEW.state OR OLD.revision IS NOT NEW.revision OR
  OLD.updated_at IS NOT NEW.updated_at OR OLD.resolved_at IS NOT NEW.resolved_at
)
BEGIN
  SELECT RAISE(ABORT, 'resolved server request is immutable');
END;

CREATE TRIGGER deny_chat_delete
BEFORE DELETE ON chats
BEGIN
  SELECT RAISE(ABORT, 'chat deletion is not admitted in ADE-0006A');
END;

CREATE TRIGGER deny_agent_session_delete
BEFORE DELETE ON agent_sessions
BEGIN
  SELECT RAISE(ABORT, 'agent session deletion is not admitted in ADE-0006A');
END;

CREATE TRIGGER deny_runtime_epoch_delete
BEFORE DELETE ON session_runtime_epochs
BEGIN
  SELECT RAISE(ABORT, 'runtime epoch deletion and reuse are not admitted');
END;

CREATE TRIGGER deny_turn_delete
BEFORE DELETE ON turns
BEGIN
  SELECT RAISE(ABORT, 'turn deletion is not admitted in ADE-0006A');
END;

CREATE TRIGGER deny_item_delete
BEFORE DELETE ON items
BEGIN
  SELECT RAISE(ABORT, 'item deletion is not admitted in ADE-0006A');
END;

CREATE TRIGGER deny_activity_delete
BEFORE DELETE ON session_activities
BEGIN
  SELECT RAISE(ABORT, 'harness activity deletion is not admitted');
END;

CREATE TRIGGER deny_server_request_delete
BEFORE DELETE ON server_requests
BEGIN
  SELECT RAISE(ABORT, 'server request deletion is not admitted');
END;

CREATE TRIGGER deny_chat_lineage_update
BEFORE UPDATE ON chat_lineage
BEGIN
  SELECT RAISE(ABORT, 'chat lineage is immutable');
END;

CREATE TRIGGER deny_chat_lineage_delete
BEFORE DELETE ON chat_lineage
BEGIN
  SELECT RAISE(ABORT, 'chat lineage is immutable');
END;

CREATE TRIGGER deny_fact_update
BEFORE UPDATE ON session_facts
BEGIN
  SELECT RAISE(ABORT, 'session facts are append-only');
END;

CREATE TRIGGER deny_fact_delete
BEFORE DELETE ON session_facts
BEGIN
  SELECT RAISE(ABORT, 'session facts are append-only');
END;
