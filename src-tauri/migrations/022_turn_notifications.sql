-- Native notice receipts, written by triggers in the same transaction as their
-- event, so a delayed projection publication cannot lose one. A Turn keeps at
-- most one receipt per outcome; each permission request keeps its own, so
-- every approval in a Turn can notify. Rows cascade with their Turn, chat or
-- request, so storage stays within those tables' retention limits.
CREATE TABLE turn_notification_receipts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
  chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('completed', 'failed', 'approval')),
  request_id TEXT REFERENCES server_requests(id) ON DELETE CASCADE,
  claimed INTEGER NOT NULL DEFAULT 0 CHECK(claimed IN (0,1)),
  CHECK((kind = 'approval') = (request_id IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX idx_turn_notification_outcome
  ON turn_notification_receipts(turn_id,kind) WHERE request_id IS NULL;
CREATE UNIQUE INDEX idx_turn_notification_request
  ON turn_notification_receipts(request_id) WHERE request_id IS NOT NULL;
CREATE INDEX idx_turn_notification_pending
  ON turn_notification_receipts(session_id,id) WHERE claimed=0;
-- Keep the outcome states in step with TurnState::is_notified_outcome.
CREATE TRIGGER turn_notification_insert AFTER INSERT ON turns
WHEN NEW.state IN ('completed','failed','deliveryUnknown')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind)
  VALUES(NEW.id,NEW.chat_id,NEW.session_id,
    CASE NEW.state WHEN 'completed' THEN 'completed' ELSE 'failed' END)
  ON CONFLICT(turn_id,kind) WHERE request_id IS NULL DO NOTHING;
END;
CREATE TRIGGER turn_notification_update AFTER UPDATE OF state ON turns
WHEN OLD.state<>NEW.state AND NEW.state IN ('completed','failed','deliveryUnknown')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind)
  VALUES(NEW.id,NEW.chat_id,NEW.session_id,
    CASE NEW.state WHEN 'completed' THEN 'completed' ELSE 'failed' END)
  ON CONFLICT(turn_id,kind) WHERE request_id IS NULL DO NOTHING;
END;
-- Permission requests leave the Turn running in ACP. Record each admission;
-- dispatch rechecks pending state so an automatic denial never notifies.
CREATE TRIGGER turn_notification_permission AFTER INSERT ON server_requests
WHEN NEW.state IN ('received','asked') AND NEW.kind IN ('permission','browserPermission')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind,request_id)
  VALUES(NEW.turn_id,NEW.chat_id,NEW.session_id,'approval',NEW.id);
END;
