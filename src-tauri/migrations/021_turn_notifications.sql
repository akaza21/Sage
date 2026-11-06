-- At most three durable event receipts per turn, bounded by the existing Turn
-- retention limits. Deletion cascades; opaque IDs are never reused for clicks.
CREATE TABLE turn_notification_receipts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
  chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('completed', 'failed', 'approval')),
  claimed INTEGER NOT NULL DEFAULT 0 CHECK(claimed IN (0,1)),
  UNIQUE(turn_id, kind)
) STRICT;
CREATE INDEX idx_turn_notification_pending
  ON turn_notification_receipts(session_id,id) WHERE claimed=0;
-- Capture the event in the same transaction as the Turn. Projection delivery
-- can lag a later Turn/state; consulting only the latest state would lose it.
CREATE TRIGGER turn_notification_insert AFTER INSERT ON turns
WHEN NEW.state IN ('completed','failed','deliveryUnknown','waitingApproval')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind)
  VALUES(NEW.id,NEW.chat_id,NEW.session_id,
    CASE NEW.state WHEN 'completed' THEN 'completed' WHEN 'waitingApproval' THEN 'approval' ELSE 'failed' END)
  ON CONFLICT(turn_id,kind) DO NOTHING;
END;
CREATE TRIGGER turn_notification_update AFTER UPDATE OF state ON turns
WHEN OLD.state<>NEW.state AND NEW.state IN ('completed','failed','deliveryUnknown','waitingApproval')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind)
  VALUES(NEW.id,NEW.chat_id,NEW.session_id,
    CASE NEW.state WHEN 'completed' THEN 'completed' WHEN 'waitingApproval' THEN 'approval' ELSE 'failed' END)
  ON CONFLICT(turn_id,kind) DO NOTHING;
END;
-- Permission requests leave the Turn running in ACP. Record their admission
-- too; dispatch rechecks pending state so an automatic denial never notifies.
CREATE TRIGGER turn_notification_permission AFTER INSERT ON server_requests
WHEN NEW.state IN ('received','asked') AND NEW.kind IN ('permission','browserPermission')
BEGIN
  INSERT INTO turn_notification_receipts(turn_id,chat_id,session_id,kind)
  VALUES(NEW.turn_id,NEW.chat_id,NEW.session_id,'approval')
  ON CONFLICT(turn_id,kind) DO NOTHING;
END;
