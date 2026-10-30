import { invokeNative } from "./nativeInvoke";

// The downloads popover's commands, kept out of `nativeBrowserPort.ts` so
// they load with the browser surface rather than with the core bundle.

export interface BrowserDownload {
  tabId: string;
  id: number;
  source: string;
  fileName: string;
  state:
    | "choosingDestination"
    | "downloading"
    | "completed"
    | "interrupted"
    | "canceled"
    | "error";
  receivedBytes: number;
  totalBytes: number | null;
}

export function browserDownloads(tabId: string): Promise<BrowserDownload[]> {
  return invokeNative("browser_downloads", { tabId });
}

export function browserCancelDownload(
  tabId: string,
  id: number,
): Promise<void> {
  return invokeNative("browser_cancel_download", { tabId, id });
}

export function browserShowDownloadInFolder(
  tabId: string,
  id: number,
): Promise<void> {
  return invokeNative("browser_show_download_in_folder", { tabId, id });
}
