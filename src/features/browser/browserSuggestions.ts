import { useWorkbenchStore } from "../../app/store";
import {
  browserStatus,
  browserSuggestedSites,
} from "../../ports/nativeBrowserPort";
import type { BrowserNavigation } from "../../ports/nativeBrowserPort";
import { browserFaviconDataUrl } from "./browserFavicon";

/** New-tab suggestions combine native history with at most eight live tabs. */
export async function browserSuggestions() {
  const sites = await browserSuggestedSites();
  const openTabs = useWorkbenchStore
    .getState()
    .tabs.filter((item) => item.kind === "browser" && item.status !== "closed")
    .slice(0, 8);
  const statuses = await Promise.allSettled(
    openTabs.map((item) => browserStatus(item.id)),
  );
  return suggestedSitesWithTitles(sites, statuses);
}

export function suggestedSitesWithTitles(
  sites: readonly string[],
  statuses: readonly PromiseSettledResult<BrowserNavigation>[],
): { url: string; title?: string | null; favicon?: string | null }[] {
  return sites.slice(0, 4).map((url) => {
    const match = statuses.find(
      (result) =>
        result.status === "fulfilled" &&
        !result.value.loading &&
        !result.value.error &&
        result.value.url === url,
    );
    return {
      url,
      title: match?.status === "fulfilled" ? match.value.title : null,
      favicon:
        match?.status === "fulfilled"
          ? browserFaviconDataUrl(match.value.favicon)
          : null,
    };
  });
}

export type BrowserSuggestedSites = ReturnType<typeof suggestedSitesWithTitles>;
