-- Who set each chat's current title. Sage's placeholder and the first-prompt
-- title may be replaced by a title the harness reports; a title the user typed
-- or a fork's derived title never is.
ALTER TABLE chats ADD COLUMN title_source TEXT NOT NULL DEFAULT 'user'
  CHECK (title_source IN ('default', 'prompt', 'harness', 'user', 'fork'));

-- title_is_default=0 has no trustworthy provenance; keep those titles as the
-- user's so nothing already shown is ever replaced.
UPDATE chats SET title_source = 'default' WHERE title_is_default = 1;

ALTER TABLE chats DROP COLUMN title_is_default;
