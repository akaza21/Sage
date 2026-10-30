CREATE TABLE chat_attention (
  chat_id TEXT PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
  session_id TEXT,
  session_state TEXT CHECK (session_state IN ('created','starting','ready','active','recovering','closing','closed','failed','incompatible')),
  turn_state TEXT CHECK (turn_state IN ('queued','sending','running','waitingModel','waitingTool','waitingApproval','waitingUser','cancelling','completed','interrupted','cancelled','failed','deliveryUnknown')),
  pending_approvals INTEGER NOT NULL DEFAULT 0 CHECK (pending_approvals BETWEEN 0 AND 4294967295),
  pending_requests INTEGER NOT NULL DEFAULT 0 CHECK (pending_requests BETWEEN 0 AND 4294967295),
  activity_position INTEGER NOT NULL DEFAULT 0 CHECK (activity_position >= 0),
  read_position INTEGER CHECK (read_position >= 0 AND read_position <= activity_position),
  terminal_position INTEGER CHECK (terminal_position >= 0 AND terminal_position <= activity_position),
  terminal_state TEXT CHECK (terminal_state IN ('completed','interrupted','cancelled','failed','deliveryUnknown')),
  forced_unread_position INTEGER CHECK (forced_unread_position >= 0 AND forced_unread_position <= activity_position),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  terminal_turn_id TEXT,
  request_identity TEXT NOT NULL DEFAULT '' CHECK (length(request_identity) <= 64)
) STRICT;
