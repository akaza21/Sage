import { useEffect, useState } from "react";
import {
  browserDownloads,
  type BrowserDownload,
} from "../../ports/nativeBrowserDownloadsPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { subscribeBrowserTab } from "./browserTabEvents";

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
    let sawEvent = false;
    return subscribeBrowserTab(
      "browser:downloads",
      tabId,
      (payload, initial) => {
        if (initial && sawEvent) return;
        sawEvent = true;
        setReported(payload);
      },
      async () => ({ tabId, downloads: await browserDownloads(tabId) }),
    );
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
