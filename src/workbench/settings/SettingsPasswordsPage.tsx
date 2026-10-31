import { useCallback, useEffect, useRef, useState } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  passwordsCancelImport,
  passwordsCopy,
  passwordsDelete,
  passwordsDeleteAll,
  passwordsDeleteImportedFile,
  passwordsUpdate,
} from "../../ports/nativePasswordsPort";
import { SavedPasswordRow } from "./SavedPasswordRow";
import {
  REVEAL_MS,
  SEARCH_DEBOUNCE_MS,
  errorText,
  importPasswordsCsv,
  importSummary,
  loadPasswords,
  revealPassword,
  siteLabel,
  type PasswordImportReport,
  type PasswordsOverview,
  type SavedPassword,
} from "./savedPasswords";
import "./settings-passwords.css";

/**
 * BF-7b-1 Settings › Passwords, after Codex ref 48. Rust keeps the vault;
 * this page holds only the current list and, for 30 seconds at most, a
 * password the user revealed after the OS confirmed them.
 */
export function SettingsPasswordsPage({ visible }: { visible: Set<string> }) {
  const desktop = isTauriRuntime();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [importing, setImporting] = useState(false);
  const cancelRequested = useRef(false);
  const importRunning = useRef(false);
  const [overview, setOverview] = useState<PasswordsOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [imported, setImported] = useState<PasswordImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [revealed, setRevealed] = useState<ReadonlyMap<string, string>>(
    () => new Map(),
  );
  const [reloads, setReloads] = useState(0);
  const hideTimers = useRef(new Map<string, number>());
  const mounted = useRef(false);
  const reload = () => setReloads((count) => count + 1);

  useEffect(() => {
    const timer = window.setTimeout(
      () => setQuery(search.trim()),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [search]);

  useEffect(() => {
    if (!desktop) return;
    let current = true;
    loadPasswords(query).then(
      (next) => {
        if (!current) return;
        setOverview(next);
        setError(null);
      },
      (cause: unknown) => {
        if (current) setError(errorText(cause));
      },
    );
    return () => {
      current = false;
    };
  }, [desktop, query, reloads]);

  const hide = useCallback((id: string) => {
    window.clearTimeout(hideTimers.current.get(id));
    hideTimers.current.delete(id);
    setRevealed((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  }, []);

  useEffect(() => {
    const timers = hideTimers.current;
    mounted.current = true;
    return () => {
      mounted.current = false;
      // Leaving the page stops an import nobody is waiting on.
      if (importRunning.current) void passwordsCancelImport();
      for (const timer of timers.values()) window.clearTimeout(timer);
      timers.clear();
    };
  }, []);

  async function run(action: () => Promise<void>): Promise<boolean> {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      return true;
    } catch (cause) {
      setError(errorText(cause));
      return false;
    } finally {
      setBusy(false);
    }
  }

  const reveal = (id: string) =>
    void run(async () => {
      const password = await revealPassword(id);
      // A page closed during the OS prompt keeps nothing it returns.
      if (!mounted.current) return;
      setRevealed((current) => new Map(current).set(id, password));
      window.clearTimeout(hideTimers.current.get(id));
      hideTimers.current.set(
        id,
        window.setTimeout(() => hide(id), REVEAL_MS),
      );
    });

  const copy = (id: string) =>
    void run(async () => {
      await passwordsCopy(id);
      setNotice(
        "Password copied. Sage clears it from the clipboard in 30 seconds.",
      );
    });

  const save = (id: string, username: string, password: string | null) =>
    run(async () => {
      await passwordsUpdate(id, username, password);
      hide(id);
      reload();
    });

  const remove = (entry: SavedPassword) => {
    const account = entry.username || "the saved password";
    if (!window.confirm(`Delete ${account} on ${siteLabel(entry.origin)}?`))
      return;
    void run(async () => {
      await passwordsDelete(entry.id);
      hide(entry.id);
      reload();
    });
  };

  const removeAll = (total: number) => {
    if (
      !window.confirm(
        `Delete all ${total} saved passwords? This can't be undone.`,
      )
    )
      return;
    void run(async () => {
      await passwordsDeleteAll();
      for (const id of revealed.keys()) hide(id);
      reload();
      setNotice("All saved passwords were deleted.");
    });
  };

  const importCsv = () => {
    cancelRequested.current = false;
    importRunning.current = true;
    setImporting(true);
    void run(async () => {
      const report = await importPasswordsCsv();
      if (report) {
        setImported(report);
        reload();
      } else if (cancelRequested.current) {
        setNotice("Import cancelled. Nothing was saved.");
      }
    }).finally(() => {
      importRunning.current = false;
      if (mounted.current) setImporting(false);
    });
  };

  const cancelImport = () => {
    cancelRequested.current = true;
    void passwordsCancelImport();
  };

  const deleteImportedFile = (report: PasswordImportReport) =>
    void run(async () => {
      await passwordsDeleteImportedFile();
      setImported(null);
      setNotice(`Deleted ${report.fileName || "the imported file"}.`);
    });

  const ready = overview?.status === "ready" ? overview : null;
  const showList = visible.has("passwords.saved");
  const showImport = visible.has("passwords.import");

  return (
    <section
      className="settings-surface__section settings-passwords"
      aria-labelledby="passwords-settings-heading"
    >
      <header className="settings-surface__section-header">
        <h2 id="passwords-settings-heading">Passwords</h2>
        <p>
          Passwords you&apos;ve saved for websites. Each use requires your
          confirmation.
        </p>
      </header>
      {!desktop ? (
        <p className="settings-surface__group-note" role="status">
          Saved passwords are available in the Sage desktop app.
        </p>
      ) : overview?.status === "unsupported" ? (
        <p className="settings-surface__group-note" role="status">
          Saved passwords are available on Windows. macOS and Linux follow once
          Sage can protect them with the system keychain.
        </p>
      ) : (
        <>
          {error ? (
            <div className="settings-surface__error" role="alert">
              <span>{error}</span>
            </div>
          ) : null}
          {notice ? (
            <p className="settings-passwords__notice" role="status">
              {notice}
            </p>
          ) : null}
          {imported ? (
            <div className="settings-passwords__imported" role="status">
              <span>
                {importSummary(imported)} The file still holds these passwords
                in plain text.
              </span>
              <button
                className="button button--compact"
                type="button"
                disabled={busy}
                onClick={() => deleteImportedFile(imported)}
              >
                Delete file
              </button>
            </div>
          ) : null}
          {showList ? (
            <label className="settings-passwords__search">
              <span className="sr-only">Search passwords</span>
              <input
                className="input"
                type="search"
                placeholder="Search passwords"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
          ) : null}
          {showList || showImport ? (
            <div className="settings-passwords__heading">
              <h3>Saved passwords ({ready?.total ?? 0})</h3>
              <div className="settings-passwords__actions">
                {importing ? (
                  <button
                    className="button button--compact"
                    type="button"
                    onClick={cancelImport}
                  >
                    Cancel import
                  </button>
                ) : showImport ? (
                  <button
                    className="button button--compact"
                    type="button"
                    disabled={busy || !ready}
                    onClick={importCsv}
                  >
                    Import CSV…
                  </button>
                ) : null}
                <button
                  className="button button--compact"
                  type="button"
                  disabled={busy}
                  onClick={reload}
                >
                  Refresh
                </button>
              </div>
            </div>
          ) : null}
          {!showList ? null : !ready ? (
            <p className="settings-passwords__empty" role="status">
              Loading saved passwords…
            </p>
          ) : ready.entries.length === 0 ? (
            <p className="settings-passwords__empty">
              {query
                ? "No saved passwords match your search."
                : "No saved passwords"}
            </p>
          ) : (
            <>
              <ul
                className="settings-passwords__list"
                aria-label="Saved passwords"
              >
                {ready.entries.map((entry) => (
                  <SavedPasswordRow
                    key={entry.id}
                    entry={entry}
                    revealed={revealed.get(entry.id)}
                    busy={busy}
                    onReveal={() => reveal(entry.id)}
                    onHide={() => hide(entry.id)}
                    onCopy={() => copy(entry.id)}
                    onSave={(username, password) =>
                      save(entry.id, username, password)
                    }
                    onDelete={() => remove(entry)}
                  />
                ))}
              </ul>
              {ready.matching > ready.entries.length ? (
                <p className="settings-passwords__empty">
                  Showing {ready.entries.length} of {ready.matching}. Search to
                  narrow the list.
                </p>
              ) : null}
            </>
          )}
          {visible.has("passwords.deleteAll") && ready && ready.total > 0 ? (
            <div className="settings-surface__footer">
              <button
                className="button button--compact button--danger"
                type="button"
                disabled={busy}
                onClick={() => removeAll(ready.total)}
              >
                Delete all saved passwords
              </button>
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
