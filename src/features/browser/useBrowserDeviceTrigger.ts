import { useState } from "react";
import type { BrowserNavigation } from "../../ports/nativeBrowserPort";

/**
 * Whether the pane's tab shows the device toolbar: all the surface keeps
 * while the toolbar itself loads lazily. The pane can show another tab
 * without remounting, and a tab's first status reopens the toolbar when Rust
 * still holds its emulation, or cannot confirm that none is left.
 */
export function useBrowserDeviceTrigger(
  tabId: string,
  status: BrowserNavigation | null,
) {
  const [device, setDevice] = useState({
    tabId,
    open: false,
    restored: false,
  });
  // The toolbar stays loaded once shown, so closing it can reset the page.
  const [loaded, setLoaded] = useState(false);
  if (device.tabId !== tabId) {
    setDevice({ tabId, open: false, restored: false });
  } else if (status && !device.restored) {
    setDevice({
      tabId,
      open: device.open || Boolean(status.device || status.deviceUnconfirmed),
      restored: true,
    });
  }
  if (device.open && !loaded) setLoaded(true);
  return {
    open: device.tabId === tabId && device.open,
    /** What the toolbar restores from the tab's status. */
    restored: status?.deviceUnconfirmed
      ? ("unconfirmed" as const)
      : (status?.device ?? null),
    loaded,
    setOpen: (open: boolean) =>
      setDevice((current) =>
        current.tabId === tabId ? { ...current, open } : current,
      ),
  };
}
