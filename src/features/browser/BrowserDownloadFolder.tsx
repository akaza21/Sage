import { useEffect, useState } from "react";

import {
  admitDownloadFolder,
  browserChooseDownloadFolder,
  browserDownloadFolder,
  browserResetDownloadFolder,
  type BrowserDownloadFolder as Folder,
} from "../../ports/nativeBrowserDownloadFolderPort";

const ROW = "browser-download-folder";
const LOAD_FAILURE = "The download folder could not be loaded.";
const SAVE_FAILURE = "The download folder could not be changed.";

function errorText(error: unknown, fallback: string): string {
  return typeof error === "string" && error.length > 0 ? error : fallback;
}

/**
 * Settings > Browser > Downloads: the folder silent downloads save to, with
 * Change (a native folder picker) and Reset (ask on the next download).
 */
export function BrowserDownloadFolder() {
  const [folder, setFolder] = useState<Folder | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    browserDownloadFolder()
      .then(
        (value) => {
          if (live) setFolder(admitDownloadFolder(value));
        },
        () => {
          if (live) setError(LOAD_FAILURE);
        },
      )
      .finally(() => {
        if (live) setLoaded(true);
      });
    return () => {
      live = false;
    };
  }, []);

  async function run(command: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setFolder(admitDownloadFolder(await command()));
    } catch (caught) {
      setError(errorText(caught, SAVE_FAILURE));
    } finally {
      setBusy(false);
    }
  }

  const status =
    folder === null
      ? null
      : folder.path === null
        ? "Not set. The next download asks where to save, and Sage remembers that folder."
        : folder.available
          ? folder.path
          : `${folder.path} is missing or read-only. The next download asks where to save.`;

  return (
    <div className="settings-surface__row">
      <div className="settings-surface__copy">
        <span id={`${ROW}-label`}>Download location</span>
        <p id={`${ROW}-description`}>
          Downloads save here without asking. Programs, scripts, and shortcuts
          (.exe, .msi, .bat, .ps1, .lnk and similar) always ask.
        </p>
        {status !== null ? (
          <small
            id={`${ROW}-path`}
            className={
              folder?.path && folder.available
                ? "settings-surface__path"
                : undefined
            }
            title={folder?.path ?? undefined}
          >
            {status}
          </small>
        ) : null}
        {error !== null ? (
          <small className="settings-surface__row-error" role="alert">
            {error}
          </small>
        ) : null}
      </div>
      <div className="settings-surface__actions">
        <button
          className="button button--compact"
          type="button"
          disabled={busy || !loaded}
          aria-describedby={
            status !== null ? `${ROW}-label ${ROW}-path` : `${ROW}-label`
          }
          onClick={() => void run(browserChooseDownloadFolder)}
        >
          Change…
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={busy || folder?.path == null}
          aria-describedby={`${ROW}-label`}
          onClick={() => void run(browserResetDownloadFolder)}
        >
          Reset
        </button>
      </div>
    </div>
  );
}
