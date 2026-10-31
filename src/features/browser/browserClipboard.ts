import { browserCaptureScreenshot } from "../../ports/nativeBrowserPort";

/** Copies a captured page PNG (base64, at most 2 MB) to the clipboard. */
export async function copyPngToClipboard(pngBase64: string): Promise<void> {
  if (!pngBase64.startsWith("iVBORw0KGgo") || pngBase64.length > 2_000_000) {
    throw new Error("invalid screenshot result");
  }
  if (!navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
    throw new Error("image clipboard unavailable");
  }
  const bytes = Uint8Array.from(atob(pngBase64), (char) => char.charCodeAt(0));
  await navigator.clipboard.write([
    new ClipboardItem({
      "image/png": new Blob([bytes], { type: "image/png" }),
    }),
  ]);
}

/** Keep clipboard work fenced to the page revision that requested it. */
export async function copyPageScreenshot(
  tabId: string,
  revision: string,
  current: () => boolean,
): Promise<boolean> {
  if (!current()) return false;
  const screenshot = await browserCaptureScreenshot(tabId, revision);
  if (!current()) return false;
  await copyPngToClipboard(screenshot.pngBase64);
  return current();
}
