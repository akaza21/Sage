import type { BrowserDownload } from "../../ports/nativeBrowserDownloadsPort";

export function progressLabel(download: BrowserDownload): string {
  switch (download.state) {
    case "choosingDestination":
      return "Choose where to save";
    case "downloading":
      return download.totalBytes && download.totalBytes > 0
        ? `${Math.min(100, Math.round((download.receivedBytes / download.totalBytes) * 100))}%`
        : "Downloading";
    case "publishing":
      return "Saving";
    case "completed":
      return "Downloaded";
    case "interrupted":
    case "error":
      return "Download failed";
    case "canceled":
      return "Canceled";
  }
}
