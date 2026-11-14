import { useCallback, useEffect, useRef, useState } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { useClearOnSuspend } from "../../ui/useClearOnSuspend";
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

export function SettingsPasswordsPage({ visible }: { visible: Set<string> }) {
  const desktop = isTauriRuntime();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [importing, setImporting] = useState(false);
  const activeImport = useRef<{ cancelRequested: boolean } | null>(null);
  const [overview, setOverview] = useState<PasswordsOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [imported, setImported] = useState<PasswordImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [deletion, setDeletion] = useState<
    { entry: SavedPassword } | { total: number } | null
  >(null);
  const [revealed, setRevealed] = useState<ReadonlyMap<string, string>>(
    () => new Map(),
  );
  const [reloads, setReloads] = useState(0);
  const hideTimers = useRef(new Map<string, number>());
  // Counts reloads of vault data; a reveal answered after one is stale.
  const vaultEpoch = useRef(0);

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

  const hideAll = useCallback(() => {
    for (const timer of hideTimers.current.values()) window.clearTimeout(timer);
    hideTimers.current.clear();
    setRevealed((current) => (current.size === 0 ? current : new Map()));
  }, []);

  // Rust may now hold a newer password than one revealed, changed by this page
  // or by another Sage window, so revealed passwords, their timers and any
  // reveal still waiting on the OS prompt are dropped first.
  const refreshVault = useCallback(
    (nextQuery?: string) => {
      vaultEpoch.current += 1;
      hideAll();
      if (nextQuery === undefined) setReloads((count) => count + 1);
      else setQuery(nextQuery);
    },
    [hideAll],
  );

  useEffect(() => {
    const next = search.trim();
    if (next === query) return;
    const timer = window.setTimeout(
      () => refreshVault(next),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [search, query, refreshVault]);

  // Settings stays alive while hidden, so on every hide, as on close, revealed
  // passwords and their timers go, a reveal still at the OS prompt is dropped,
  // and a running import is cancelled.
  useClearOnSuspend(
    useCallback(() => {
      const request = activeImport.current;
      if (request) {
        request.cancelRequested = true;
        void passwordsCancelImport();
      }
      vaultEpoch.current += 1;
      hideAll();
    }, [hideAll]),
  );

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
      const epoch = vaultEpoch.current;
      const password = await revealPassword(id);
      // A page hidden or closed during the OS prompt keeps nothing it
      // returns, and a reload since then may have replaced the password.
      if (vaultEpoch.current !== epoch) return;
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
        "Password copied. Sage will try to clear it from the clipboard in 30 seconds.",
      );
    });

  const save = (id: string, username: string, password: string | null) =>
    run(async () => {
      await passwordsUpdate(id, username, password);
      refreshVault();
    });

  const remove = (entry: SavedPassword) => {
    void run(async () => {
      await passwordsDelete(entry.id);
      refreshVault();
    });
  };

  const removeAll = () => {
    void run(async () => {
      await passwordsDeleteAll();
      refreshVault();
      setNotice("All saved passwords were deleted.");
    });
  };

  const importCsv = () => {
    const request = { cancelRequested: false };
    activeImport.current = request;
    setImporting(true);
    void run(async () => {
      const report = await importPasswordsCsv().finally(() => {
        activeImport.current = null;
        setNotice(null);
      });
      if (report) {
        setImported(report);
        refreshVault();
      } else if (request.cancelRequested) {
        setNotice("Import cancelled. Nothing was saved.");
      }
    }).finally(() => setImporting(false));
  };

  const cancelImport = () => {
    const request = activeImport.current;
    if (!request) return;
    request.cancelRequested = true;
    void passwordsCancelImport().then(
      (accepted) => {
        if (activeImport.current === request && accepted === false) {
          setNotice(
            "The import can no longer be cancelled. Its results will appear when it finishes.",
          );
        }
      },
      (cause: unknown) => {
        if (activeImport.current === request) setError(errorText(cause));
      },
    );
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
                  onClick={() => refreshVault()}
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
                    onDelete={() => setDeletion({ entry })}
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
                onClick={() => setDeletion({ total: ready.total })}
              >
                Delete all saved passwords
              </button>
            </div>
          ) : null}
        </>
      )}
      {deletion ? (
        <ConfirmDialog
          title={
            "entry" in deletion
              ? `Delete ${deletion.entry.username || "the saved password"} on ${siteLabel(deletion.entry.origin)}?`
              : `Delete all ${deletion.total} saved passwords?`
          }
          description={
            "entry" in deletion
              ? "This saved password will be permanently deleted from Sage. This can't be undone."
              : "Every saved password in Sage will be permanently deleted, including passwords hidden by your search. This can't be undone."
          }
          confirmLabel={
            "entry" in deletion
              ? "Delete password"
              : "Delete all saved passwords"
          }
          onCancel={() => setDeletion(null)}
          onConfirm={() => {
            setDeletion(null);
            if ("entry" in deletion) remove(deletion.entry);
            else removeAll();
          }}
        />
      ) : null}
    </section>
  );
}
