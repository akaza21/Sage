import { FileText, FolderOpen, X } from "../../ui/leanIcons";

import { useState } from "react";
import {
  browserCancelDownload,
  browserShowDownloadInFolder,
  type BrowserDownload,
} from "../../ports/nativeBrowserDownloadsPort";
import { progressLabel } from "./browserDownloadLabel";
import { isActiveDownload } from "./useBrowserDownloads";

function sourceHost(source: string): string {
  try {
    return new URL(source).host || source;
  } catch {
    return source;
  }
}

/** The downloads popover's body (Codex toolbar ⬇), newest first. */
export default function BrowserDownloadsPanel({
  tabId,
  downloads,
}: {
  tabId: string;
  downloads: readonly BrowserDownload[];
}) {
  const [error, setError] = useState<string | null>(null);
  const fail = (cause: unknown) => setError(String(cause).slice(0, 256));
  return (
    <>
      <h2 className="browser-downloads__title">Downloads</h2>
      {downloads.length ? (
        <ul className="browser-downloads__list">
          {[...downloads].reverse().map((download) => (
            <li key={download.id} className="browser-downloads__item">
              <FileText size={16} aria-hidden="true" />
              <span
                className="browser-downloads__name"
                title={download.fileName}
              >
                {download.fileName}
              </span>
              <span className="browser-downloads__status">
                {progressLabel(download)} · {sourceHost(download.source)}
              </span>
              {isActiveDownload(download) ? (
                <button
                  type="button"
                  aria-label={`Cancel ${download.fileName}`}
                  title="Cancel"
                  onClick={() =>
                    void browserCancelDownload(tabId, download.id).catch(fail)
                  }
                >
                  <X size={14} aria-hidden="true" />
                </button>
              ) : null}
              {download.state === "completed" ? (
                <button
                  type="button"
                  aria-label={`Show ${download.fileName} in folder`}
                  title="Show in folder"
                  onClick={() =>
                    void browserShowDownloadInFolder(tabId, download.id).catch(
                      fail,
                    )
                  }
                >
                  <FolderOpen size={14} aria-hidden="true" />
                </button>
              ) : null}
              {download.state === "downloading" && download.totalBytes ? (
                <progress
                  value={Math.min(download.receivedBytes, download.totalBytes)}
                  max={download.totalBytes}
                  aria-label={`${download.fileName} download progress`}
                />
              ) : null}
            </li>
          ))}
        </ul>
      ) : (
        <p className="browser-downloads__empty">No downloads yet</p>
      )}
      {error ? (
        <p className="browser-downloads__error" role="alert">
          {error}
        </p>
      ) : null}
    </>
  );
}
