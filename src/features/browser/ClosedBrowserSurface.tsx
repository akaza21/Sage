import { useWorkbenchStore } from "../../app/store";

export function ClosedBrowserSurface({ tabId }: { tabId: string }) {
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  return (
    <section
      aria-label="Browser page"
      className="browser-surface browser-surface--closed"
    >
      <p>
        This browser session closed when Sage restarted. Its page was not
        restored.
      </p>
      <button type="button" onClick={() => void openBrowser()}>
        Open new browser
      </button>
      <button type="button" onClick={() => closeTab(tabId)}>
        Close tab
      </button>
    </section>
  );
}
