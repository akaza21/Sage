import { useEffect, useState } from "react";
import {
  browserDownloads,
  type BrowserDownload,
} from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";

export function isActiveDownload(download: BrowserDownload): boolean {
  return (
    download.state === "choosingDestination" || download.state === "downloading"
  );
}

const NONE: BrowserDownload[] = [];

/** The tab's downloads as Rust reports them, newest last. */
export function useBrowserDownloads(tabId: string): BrowserDownload[] {
  const [reported, setReported] = useState<{
    tabId: string;
    downloads: BrowserDownload[];
  } | null>(null);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let mounted = true;
    let sawEvent = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const stop = await listen<{
          tabId: string;
          downloads: BrowserDownload[];
        }>("browser:downloads", (event) => {
          if (mounted && event.payload.tabId === tabId) {
            sawEvent = true;
            setReported(event.payload);
          }
        });
        if (!mounted) return stop();
        unlisten = stop;
        const initial = await browserDownloads(tabId);
        if (mounted && !sawEvent) setReported({ tabId, downloads: initial });
      })
      .catch(() => undefined);
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [tabId]);
  return reported?.tabId === tabId ? reported.downloads : NONE;
}

/** Share of the active downloads' known bytes received, or null if none. */
export function activeDownloadProgress(
  downloads: readonly BrowserDownload[],
): number | null {
  let received = 0;
  let total = 0;
  for (const download of downloads) {
    if (download.state !== "downloading" || !download.totalBytes) continue;
    received += Math.min(download.receivedBytes, download.totalBytes);
    total += download.totalBytes;
  }
  return total > 0 ? received / total : null;
}
