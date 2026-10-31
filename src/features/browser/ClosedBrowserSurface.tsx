import { useRef, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";

export function ClosedBrowserSurface({ tabId }: { tabId: string }) {
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const pending = useRef(false);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  async function open() {
    if (pending.current) return;
    pending.current = true;
    setOpening(true);
    setError("");
    try {
      const before = useWorkbenchStore.getState();
      await openBrowser();
      const after = useWorkbenchStore.getState();
      if (
        after.projectGeneration === before.projectGeneration &&
        !after.tabs.some(
          (tab) =>
            tab.kind === "browser" &&
            !before.tabs.some((old) => old.id === tab.id),
        )
      )
        setError(
          after.error ?? "A new browser could not be opened. Try again.",
        );
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    } finally {
      pending.current = false;
      setOpening(false);
    }
  }
  return (
    <section
      aria-label="Browser page"
      className="browser-surface browser-surface--closed"
    >
      <ResourceStateNotice
        state="idle"
        title="Browser session closed"
        detail="This browser session closed when Sage restarted. Its page was not restored."
      />
      <div className="browser-surface__recovery-actions">
        <button
          className="button button--compact"
          type="button"
          disabled={opening}
          onClick={() => void open()}
        >
          {opening ? "Opening browser…" : "Open new browser"}
        </button>
        <button
          className="button button--compact"
          type="button"
          onClick={() => closeTab(tabId)}
        >
          Close tab
        </button>
      </div>
      {error ? (
        <ResourceStateNotice
          state="error"
          title="Could not open browser"
          detail={error}
        />
      ) : null}
    </section>
  );
}

import type * as NewTabModule from "./BrowserNewTab";
// Chromium remembers a rejected module URL. An explicit retry has a separate,
// bounded module identity; it still renders this tab and never opens another.
const recoveryNewTab = import.meta.glob<typeof NewTabModule>(
  "./BrowserNewTab.tsx",
  { query: "?sage-browser-recovery" },
);
let newTabImportFailed = false;
export const loadBrowserNewTab = async () => {
  try {
    const module = await (newTabImportFailed
      ? recoveryNewTab["./BrowserNewTab.tsx"]()
      : import("./BrowserNewTab"));
    return { default: module.BrowserNewTab };
  } catch (cause) {
    newTabImportFailed = true;
    throw cause;
  }
};
