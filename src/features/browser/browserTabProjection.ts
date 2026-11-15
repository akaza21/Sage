import { useWorkbenchStore } from "../../app/store";
import type { BrowserNavigation } from "../../ports/nativeBrowserPort";

export function projectBrowserTabStatus(
  tabId: string,
  status: BrowserNavigation,
) {
  const store = useWorkbenchStore.getState();
  const { favicon, url, title, revision, loading, error } = status;
  store.updateBrowserTabFavicon(tabId, favicon, revision);
  if (!loading && !error && url)
    store.updateBrowserTabTitle(tabId, url, title, revision);
}
