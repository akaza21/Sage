import { useEffect, useId, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Notice } from "../../ui/Notice";
import {
  clearControlMounted,
  mountClearControl,
  useClearProgress,
} from "./browserClearProgress";
import { reconcileNativeClosedBrowserTabs } from "../../app/tabClosing";
import {
  browserClearData,
  type BrowserClearDataCompletion,
} from "../../ports/nativeBrowserPort";

const setBusy = (busy: boolean) => useClearProgress.setState({ busy });
/** One place explains a failure: its control, or the workbench when none is mounted. */
const setError = (error: string | null) => {
  if (error && !clearControlMounted()) {
    useClearProgress.setState({ error: null });
    useWorkbenchStore.setState({ error });
  } else useClearProgress.setState({ error });
};
const setSuccess = (success: string | null) =>
  useClearProgress.setState({ success });

export function BrowserClearData({
  menuItem = false,
  buttonClassName,
  describedBy,
}: {
  menuItem?: boolean;
  buttonClassName?: string;
  describedBy?: string;
}) {
  const { busy, error, success, owner } = useClearProgress();
  const [confirming, setConfirming] = useState(false);
  const id = useId();
  useEffect(() => mountClearControl(id), [id]);

  async function clear() {
    if (useClearProgress.getState().busy) return;
    setConfirming(false);
    useClearProgress.setState({ owner: id });
    setBusy(true);
    setError(null);
    setSuccess(null);
    let unlisten: (() => void) | undefined;
    let pending = false;
    let completion: BrowserClearDataCompletion | null = null;
    const finish = (outcome: BrowserClearDataCompletion) => {
      unlisten?.();
      unlisten = undefined;
      setBusy(false);
      setError(outcome.error);
      setSuccess(outcome.cleared ? "Browsing data cleared." : null);
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
        useClearProgress.setState({ error: result.error });
        if (completion) finish(completion);
        return;
      }
      unlisten();
      unlisten = undefined;
      if (!result.cleared)
        setError(result.error ?? "Browser data could not be cleared.");
      else setSuccess("Browsing data cleared.");
    } catch (cause) {
      unlisten?.();
      setError(String(cause).slice(0, 512));
    } finally {
      if (!pending) setBusy(false);
    }
  }

  const message = error ?? success;
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
      {owner === id && message ? (
        <Notice
          identity={`browser-clear-data:${busy ? "pending" : error ? "failed" : "cleared"}`}
          severity={error && !busy ? "warning" : "info"}
          message={message}
          progress={busy || !error}
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
