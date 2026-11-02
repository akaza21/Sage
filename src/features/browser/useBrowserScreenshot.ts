import { useEffect, useRef, useState } from "react";
import { browserCaptureScreenshot } from "../../ports/nativeBrowserPort";
import { copyPngToClipboard } from "./browserClipboard";

const COPIED = "Screenshot copied to clipboard";

/** Take a screenshot: copies the page to the clipboard and reports it.
 * Bumping `epoch` (a navigation) drops a capture still in flight. */
export function useBrowserScreenshot(
  tabId: string,
  onError: (message: string | null) => void,
) {
  const epoch = useRef(0);
  const [status, setStatus] = useState<string | null>(null);
  useEffect(() => {
    if (status !== COPIED) return;
    const timeout = setTimeout(() => setStatus(null), 5_000);
    return () => clearTimeout(timeout);
  }, [status]);
  async function copy(revision: string | undefined) {
    if (!revision) return;
    const started = epoch.current;
    onError(null);
    setStatus("Capturing screenshot…");
    try {
      const screenshot = await browserCaptureScreenshot(tabId, revision);
      if (started !== epoch.current) return;
      await copyPngToClipboard(screenshot.pngBase64);
      if (started !== epoch.current) return;
      setStatus(COPIED);
    } catch (cause) {
      if (started !== epoch.current) return;
      setStatus(null);
      onError(`Screenshot could not be copied: ${String(cause).slice(0, 384)}`);
    }
  }
  return { status, setStatus, epoch, copy };
}
