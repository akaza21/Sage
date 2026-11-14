import { useEffect, useState } from "react";
import {
  browserDownloads,
  type BrowserDownload,
} from "../../ports/nativeBrowserDownloadsPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { subscribeBrowserTab } from "./browserTabEvents";

export function isActiveDownload(download: BrowserDownload): boolean {
  return (
    download.state === "choosingDestination" ||
    download.state === "downloading" ||
    download.state === "publishing"
  );
}

const NONE: BrowserDownload[] = [];

export function useBrowserDownloads(tabId: string): BrowserDownload[] {
  const [reported, setReported] = useState<{
    tabId: string;
    downloads: BrowserDownload[];
  } | null>(null);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let events = 0;
    let readEpoch = 0;
    return subscribeBrowserTab(
      "browser:downloads",
      tabId,
      (payload, initial) => {
        if (initial && readEpoch !== events) return;
        if (!initial) events += 1;
        setReported(payload);
      },
      async () => {
        readEpoch = events;
        return { tabId, downloads: await browserDownloads(tabId) };
      },
    );
  }, [tabId]);
  return reported?.tabId === tabId ? reported.downloads : NONE;
}

/** Fraction (0–1) of the active downloads' known bytes, or null if none. */
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
