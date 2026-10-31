import { useCallback, useEffect, useRef, useState } from "react";
import {
  browserHistoryPage,
  browserHistoryRemove,
  type BrowserHistoryCursor,
  type BrowserVisit,
} from "../../ports/nativeBrowserPort";

/** Visits fetched per page. */
export const HISTORY_PAGE_SIZE = 50;
/** The most visits the page holds; older ones need a narrower search. */
export const HISTORY_MAX_LOADED = 500;
/** Typing settles this long before a search runs. */
export const HISTORY_SEARCH_DELAY_MS = 200;

export interface BrowsingHistoryView {
  visits: readonly BrowserVisit[];
  /** True while the first page of the current search loads. */
  loading: boolean;
  /** True while a further page loads. */
  loadingMore: boolean;
  /** Another page exists and the loaded list is below its bound. */
  canLoadMore: boolean;
  /** Another page exists but the loaded list reached its bound. */
  truncated: boolean;
  error: string | null;
  removing: ReadonlySet<number>;
  loadMore: () => void;
  remove: (visit: BrowserVisit) => void;
}

function message(cause: unknown): string {
  return String(cause).slice(0, 512);
}

/**
 * The History page's data: newest-first visits matching `search`, paged
 * from Rust and bounded here. A response for an older search is dropped.
 */
export function useBrowsingHistory(search: string): BrowsingHistoryView {
  const [visits, setVisits] = useState<BrowserVisit[]>([]);
  const [next, setNext] = useState<BrowserHistoryCursor | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ReadonlySet<number>>(new Set());
  const epoch = useRef(0);

  useEffect(() => {
    const current = ++epoch.current;
    setLoading(true);
    setLoadingMore(false);
    setError(null);
    const run = () => {
      browserHistoryPage({ search, before: null, limit: HISTORY_PAGE_SIZE })
        .then((page) => {
          if (current !== epoch.current) return;
          setVisits(page.visits);
          setNext(page.next);
        })
        .catch((cause) => {
          if (current !== epoch.current) return;
          setVisits([]);
          setNext(null);
          setError(message(cause));
        })
        .finally(() => {
          if (current === epoch.current) setLoading(false);
        });
    };
    // The unfiltered list loads at once; a search waits for typing to settle.
    if (!search) {
      run();
      return;
    }
    const timer = setTimeout(run, HISTORY_SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [search]);

  const canLoadMore = next !== null && visits.length < HISTORY_MAX_LOADED;
  const loadMore = useCallback(() => {
    if (!next || loading || loadingMore) return;
    if (visits.length >= HISTORY_MAX_LOADED) return;
    const current = epoch.current;
    setLoadingMore(true);
    browserHistoryPage({ search, before: next, limit: HISTORY_PAGE_SIZE })
      .then((page) => {
        if (current !== epoch.current) return;
        setVisits((loaded) =>
          [...loaded, ...page.visits].slice(0, HISTORY_MAX_LOADED),
        );
        setNext(page.next);
      })
      .catch((cause) => {
        if (current === epoch.current) setError(message(cause));
      })
      .finally(() => {
        if (current === epoch.current) setLoadingMore(false);
      });
  }, [loading, loadingMore, next, search, visits.length]);

  const remove = useCallback((visit: BrowserVisit) => {
    setRemoving((ids) => new Set(ids).add(visit.id));
    setError(null);
    browserHistoryRemove([visit.id])
      .then(() =>
        setVisits((loaded) => loaded.filter((item) => item.id !== visit.id)),
      )
      .catch((cause) => setError(message(cause)))
      .finally(() =>
        setRemoving((ids) => {
          const remaining = new Set(ids);
          remaining.delete(visit.id);
          return remaining;
        }),
      );
  }, []);

  return {
    visits,
    loading,
    loadingMore,
    canLoadMore,
    truncated: next !== null && !canLoadMore,
    error,
    removing,
    loadMore,
    remove,
  };
}
