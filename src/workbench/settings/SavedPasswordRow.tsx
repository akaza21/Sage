import { useState, type FormEvent } from "react";

import { siteLabel, type SavedPassword } from "./savedPasswords";

export function SavedPasswordRow({
  entry,
  revealed,
  busy,
  onReveal,
  onHide,
  onCopy,
  onSave,
  onDelete,
}: {
  entry: SavedPassword;
  revealed: string | undefined;
  busy: boolean;
  onReveal: () => void;
  onHide: () => void;
  onCopy: () => void;
  onSave: (username: string, password: string | null) => Promise<boolean>;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const site = siteLabel(entry.origin);
  const account = entry.username || "No username";

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const password = String(form.get("password") ?? "");
    if (await onSave(String(form.get("username") ?? ""), password || null))
      setEditing(false);
  }

  if (editing) {
    return (
      <li className="settings-passwords__row settings-passwords__row--editing">
        <form
          className="settings-passwords__edit"
          aria-label={`Edit ${account} on ${site}`}
          onSubmit={(event) => void save(event)}
        >
          <span className="settings-passwords__site">{site}</span>
          <label>
            <span>Username</span>
            <input
              className="input"
              name="username"
              defaultValue={entry.username}
              autoComplete="off"
              spellCheck={false}
              autoFocus
            />
          </label>
          <label>
            <span>New password</span>
            <input
              className="input"
              name="password"
              type="password"
              autoComplete="new-password"
              placeholder="Leave blank to keep the current password"
            />
          </label>
          <div className="settings-passwords__actions">
            <button
              className="button button--compact"
              type="button"
              onClick={() => setEditing(false)}
            >
              Cancel
            </button>
            <button
              className="button button--compact button--primary"
              type="submit"
              disabled={busy}
            >
              Save
            </button>
          </div>
        </form>
      </li>
    );
  }

  return (
    <li className="settings-passwords__row">
      <div className="settings-passwords__identity">
        <span className="settings-passwords__site" title={entry.origin}>
          {site}
        </span>
        <span
          className="settings-passwords__username"
          data-empty={entry.username ? undefined : true}
        >
          {account}
        </span>
        {revealed !== undefined ? (
          <code
            className="settings-passwords__secret"
            aria-label={`Password for ${account} on ${site}`}
          >
            {revealed}
          </code>
        ) : null}
      </div>
      <div
        className="settings-passwords__actions"
        role="group"
        aria-label={`${account} on ${site}`}
      >
        <button
          className="button button--compact"
          type="button"
          disabled={busy}
          onClick={revealed === undefined ? onReveal : onHide}
        >
          {revealed === undefined ? "Show" : "Hide"}
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={busy}
          onClick={onCopy}
        >
          Copy
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={busy}
          onClick={() => setEditing(true)}
        >
          Edit
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={busy}
          onClick={onDelete}
        >
          Delete
        </button>
      </div>
    </li>
  );
}
