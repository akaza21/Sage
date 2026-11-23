import { useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Notice } from "../../ui/Notice";
import { useClearProgress } from "./browserClearProgress";
import { reconcileNativeClosedBrowserTabs } from "../../app/tabClosing";
import {
  browserClearData,
  type BrowserClearDataCompletion,
} from "../../ports/nativeBrowserPort";

const setBusy = (busy: boolean) => useClearProgress.setState({ busy });
// The status bar outlives the browser tabs closed by this operation.
const setOutcome = (browserClearNotice: string | null) =>
  useWorkbenchStore.setState({ browserClearNotice });

export function BrowserClearData({
  menuItem = false,
  buttonClassName,
  describedBy,
}: {
  menuItem?: boolean;
  buttonClassName?: string;
  describedBy?: string;
}) {
  const busy = useClearProgress((state) => state.busy);
  const [confirming, setConfirming] = useState(false);

  async function clear() {
    if (useClearProgress.getState().busy) return;
    setConfirming(false);
    setBusy(true);
    setOutcome(null);
    let unlisten: (() => void) | undefined;
    let pending = false;
    let completion: BrowserClearDataCompletion | null = null;
    const finish = (outcome: BrowserClearDataCompletion) => {
      unlisten?.();
      unlisten = undefined;
      setBusy(false);
      setOutcome(
        outcome.error ??
          (outcome.cleared
            ? "Browsing data cleared."
            : "Browser data could not be cleared."),
      );
    };
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<BrowserClearDataCompletion>(
        "browser:clear-data-complete",
        (event) => {
          completion = event.payload;
          if (pending) finish(event.payload);
        },
      );
      const result = await browserClearData();
      reconcileNativeClosedBrowserTabs(
        useWorkbenchStore.setState,
        useWorkbenchStore.getState,
        result.closedTabIds,
      );
      if (result.pending) {
        pending = true;
        if (completion) finish(completion);
        return;
      }
      finish(result);
    } catch (cause) {
      unlisten?.();
      setBusy(false);
      setOutcome(String(cause).slice(0, 512));
    }
  }

  return (
    <div
      className={
        menuItem
          ? "browser-clear-data browser-clear-data--menu"
          : "browser-clear-data"
      }
    >
      <button
        className={buttonClassName}
        type="button"
        role={menuItem ? "menuitem" : undefined}
        disabled={busy}
        aria-describedby={describedBy}
        onClick={() => setConfirming(true)}
      >
        {busy ? "Clearing browser data…" : "Clear browsing data"}
      </button>
      {busy ? (
        <Notice
          identity="browser-clear-data:pending"
          severity="info"
          message="Browser data is still clearing; new browser tabs are blocked until it finishes."
          progress
        />
      ) : null}
      {confirming ? (
        <ConfirmDialog
          title="Clear browsing data?"
          description="Clear browser history, cookies, and site storage? All browser tabs will close and agent access will be revoked. Sage and preview data stay intact."
          confirmLabel="Clear browsing data"
          onCancel={() => setConfirming(false)}
          onConfirm={() => void clear()}
        />
      ) : null}
    </div>
  );
}
