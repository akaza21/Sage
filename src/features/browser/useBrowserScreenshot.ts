import { useEffect, useRef, useState } from "react";

const COPIED = "Screenshot copied to clipboard";

/** Callers bump `epoch` on navigation, dropping a capture still in flight. */
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
      const { copyPageScreenshot } = await import("./browserClipboard");
      if (
        await copyPageScreenshot(
          tabId,
          revision,
          () => started === epoch.current,
        )
      )
        setStatus(COPIED);
    } catch (cause) {
      if (started !== epoch.current) return;
      setStatus(null);
      onError(`Screenshot could not be copied: ${String(cause).slice(0, 384)}`);
    }
  }
  return { status, setStatus, epoch, copy };
}
