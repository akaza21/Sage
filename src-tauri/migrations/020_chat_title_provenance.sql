-- Existing titles have no trustworthy provenance; preserve them conservatively.
ALTER TABLE chats ADD COLUMN title_is_default INTEGER NOT NULL DEFAULT 0
  CHECK (title_is_default IN (0, 1));
