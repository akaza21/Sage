import { Download, X } from "../../ui/leanIcons";

import { useEffect, useState } from "react";
import {
  browserDownloads,
  browserCancelDownload,
  browserShowDownloadInFolder,
  type BrowserDownload,
} from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { progressLabel } from "./browserDownloadLabel";

export function BrowserDownloads({
  tabId,
  open = false,
  onClose,
}: {
  tabId: string;
  open?: boolean;
  onClose?: () => void;
}) {
  const [downloads, setDownloads] = useState<BrowserDownload[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let mounted = true;
    let sawEvent = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        unlisten = await listen<{
          tabId: string;
          downloads: BrowserDownload[];
        }>("browser:downloads", (event) => {
          if (mounted && event.payload.tabId === tabId) {
            sawEvent = true;
            setDownloads(event.payload.downloads);
          }
        });
        const initial = await browserDownloads(tabId);
        if (mounted && !sawEvent) setDownloads(initial);
      })
      .catch(() => undefined);
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [tabId]);

  if (downloads.length === 0 && !open) return null;
  const show = open || expanded;
  const active = downloads.filter(
    (download) =>
      download.state === "choosingDestination" ||
      download.state === "downloading",
  ).length;
  return (
    <aside className="browser-downloads" aria-label="Browser downloads">
      <button
        className="browser-downloads__toggle"
        type="button"
        aria-expanded={show}
        onClick={() =>
          open ? onClose?.() : setExpanded((current) => !current)
        }
      >
        <Download size={15} aria-hidden="true" />
        <span>Downloads{active ? ` · ${active} active` : ""}</span>
        <span aria-hidden="true">{show ? "▾" : "▸"}</span>
      </button>
      {show ? (
        downloads.length ? (
          <ul className="browser-downloads__list">
            {[...downloads].reverse().map((download) => (
              <li key={download.id}>
                <span
                  className="browser-downloads__name"
                  title={download.fileName}
                >
                  {download.fileName}
                </span>
                <span className="browser-downloads__status">
                  {progressLabel(download)}
                </span>
                {download.state === "choosingDestination" ||
                download.state === "downloading" ? (
                  <button
                    type="button"
                    aria-label={`Cancel ${download.fileName}`}
                    onClick={() =>
                      void browserCancelDownload(tabId, download.id).catch(
                        (cause) => setError(String(cause).slice(0, 256)),
                      )
                    }
                  >
                    <X size={13} aria-hidden="true" />
                  </button>
                ) : null}
                {download.state === "completed" ? (
                  <button
                    type="button"
                    aria-label={`Show ${download.fileName} in folder`}
                    onClick={() =>
                      void browserShowDownloadInFolder(
                        tabId,
                        download.id,
                      ).catch((cause) => setError(String(cause).slice(0, 256)))
                    }
                  >
                    Show in folder
                  </button>
                ) : null}
                <span className="browser-downloads__source">
                  {download.source}
                </span>
                {download.state === "downloading" && download.totalBytes ? (
                  <progress
                    value={download.receivedBytes}
                    max={download.totalBytes}
                    aria-label={`${download.fileName} download progress`}
                  />
                ) : null}
              </li>
            ))}
          </ul>
        ) : (
          <p>No downloads yet.</p>
        )
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </aside>
  );
}
