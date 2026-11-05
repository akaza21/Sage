import { invokeNative } from "./nativeInvoke";

/** One visit in the global browsing history (BF-7c). */
export interface BrowserVisit {
  id: number;
  url: string;
  title: string | null;
  /** Milliseconds since the Unix epoch. */
  visitedAt: number;
}

/** Where the next page starts: strictly older than this visit. */
export interface BrowserHistoryCursor {
  visitedAt: number;
  id: number;
}

export interface BrowserHistoryPage {
  visits: BrowserVisit[];
  next: BrowserHistoryCursor | null;
}

export function browserHistoryPage(query: {
  search: string;
  before: BrowserHistoryCursor | null;
  limit: number;
}): Promise<BrowserHistoryPage> {
  return invokeNative("browser_history_page", { query });
}

export function browserHistoryRemove(ids: readonly number[]): Promise<number> {
  return invokeNative("browser_history_remove", { ids });
}

/** This tab's navigation stack for Settings; persistent visits use browserHistoryPage. */
export function browserHistory(tabId: string): Promise<string[]> {
  return invokeNative("browser_history", { tabId });
}

export function browserSuggestedSites(): Promise<string[]> {
  return invokeNative("browser_suggested_sites");
}
