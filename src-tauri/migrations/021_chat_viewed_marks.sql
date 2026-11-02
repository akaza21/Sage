-- When the newest finished turn the user has seen in each chat finished, so
-- a chat stays read across a reload until another turn finishes.
ALTER TABLE chats ADD COLUMN viewed_turn_at INTEGER
  CHECK (viewed_turn_at IS NULL OR viewed_turn_at >= 0);
