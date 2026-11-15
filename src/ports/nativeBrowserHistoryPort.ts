import { invokeNative } from "./nativeInvoke";

export interface BrowserVisit {
  id: number;
  url: string;
  title: string | null;
  /** Milliseconds since the Unix epoch. */
  visitedAt: number;
}

/** The next page is strictly older than this visit. */
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

export function browserHistory(tabId: string): Promise<string[]> {
  return invokeNative("browser_history", { tabId });
}

export function browserSuggestedSites(): Promise<string[]> {
  return invokeNative("browser_suggested_sites");
}
