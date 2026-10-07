import { useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { reconcileNativeClosedBrowserTabs } from "../../app/tabClosing";
import {
  browserClearData,
  type BrowserClearDataCompletion,
} from "../../ports/nativeBrowserPort";

export function BrowserClearData({ menuItem = false }: { menuItem?: boolean }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  async function clear() {
    if (
      !window.confirm(
        "Clear browser history, cookies, and site storage? All browser tabs will close and agent access will be revoked. Sage and preview data stay intact.",
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    setSuccess(null);
    useWorkbenchStore.setState({ browserClearNotice: null });
    let unlisten: (() => void) | undefined;
    let pending = false;
    let completion: BrowserClearDataCompletion | null = null;
    const finish = (outcome: BrowserClearDataCompletion) => {
      unlisten?.();
      unlisten = undefined;
      setBusy(false);
      setError(outcome.error);
      const notice = outcome.cleared ? "Browsing data cleared." : null;
      setSuccess(notice);
      useWorkbenchStore.setState({
        error: outcome.error,
        browserClearNotice: notice,
      });
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
        setError(result.error);
        useWorkbenchStore.setState({ error: result.error });
        if (completion) finish(completion);
        return;
      }
      unlisten();
      unlisten = undefined;
      if (!result.cleared) {
        const message = result.error ?? "Browser data could not be cleared.";
        setError(message);
        useWorkbenchStore.setState({ error: message });
      } else {
        const notice = "Browsing data cleared.";
        setSuccess(notice);
        useWorkbenchStore.setState({ browserClearNotice: notice });
      }
    } catch (cause) {
      unlisten?.();
      const message = String(cause).slice(0, 512);
      setError(message);
      useWorkbenchStore.setState({ error: message });
    } finally {
      if (!pending) setBusy(false);
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
        type="button"
        role={menuItem ? "menuitem" : undefined}
        disabled={busy}
        onClick={() => void clear()}
      >
        {busy ? "Clearing browser data…" : "Clear browsing data"}
      </button>
      {error ? <span role="alert">{error}</span> : null}
      {success ? <span role="status">{success}</span> : null}
    </div>
  );
}
