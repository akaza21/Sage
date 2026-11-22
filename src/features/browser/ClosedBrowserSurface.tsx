import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { Notice } from "../../ui/Notice";

export default function ClosedBrowserSurface({ tabId }: { tabId: string }) {
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const [error, setError] = useState<string | null>(null);
  return (
    <section
      aria-label="Browser page"
      className="browser-surface browser-surface--closed"
    >
      <p>
        This browser session closed when Sage restarted. Its page was not
        restored.
      </p>
      {error ? (
        <Notice
          identity={`browser:${tabId}:reopen`}
          severity="warning"
          message={error}
        />
      ) : null}
      <button
        type="button"
        onClick={() => {
          setError(null);
          void openBrowser().then((message) => setError(message ?? null));
        }}
      >
        Open new browser
      </button>
      <button type="button" onClick={() => closeTab(tabId)}>
        Close tab
      </button>
    </section>
  );
}
