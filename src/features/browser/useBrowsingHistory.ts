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
  /** True until the first page of the current search has answered. */
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

/** What Rust answered for one search. */
interface Listing {
  search: string;
  visits: BrowserVisit[];
  next: BrowserHistoryCursor | null;
  error: string | null;
}

function message(cause: unknown): string {
  return String(cause).slice(0, 512);
}

/**
 * The History page's data: newest-first visits matching `search`, paged
 * from Rust and bounded here. Everything shown belongs to the search it
 * was answered for, so an answer for an older search is never shown.
 */
export function useBrowsingHistory(search: string): BrowsingHistoryView {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loadingMore, setLoadingMore] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ReadonlySet<number>>(new Set());
  const latest = useRef(search);

  useEffect(() => {
    latest.current = search;
    let live = true;
    const run = () => {
      browserHistoryPage({ search, before: null, limit: HISTORY_PAGE_SIZE })
        .then(
          (page) => ({ search, ...page, error: null }),
          (cause) => ({
            search,
            visits: [],
            next: null,
            error: message(cause),
          }),
        )
        .then((answer) => {
          if (live) {
            setListing(answer);
            setActionError(null);
          }
        });
    };
    // The unfiltered list loads at once; a search waits for typing to settle.
    if (!search) run();
    const timer = search ? setTimeout(run, HISTORY_SEARCH_DELAY_MS) : 0;
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [search]);

  const current = listing?.search === search ? listing : null;
  const visits = current?.visits ?? [];
  const next = current?.next ?? null;
  const canLoadMore = next !== null && visits.length < HISTORY_MAX_LOADED;
  const busy = loadingMore === search;

  const loadMore = useCallback(() => {
    if (!current || !current.next || busy) return;
    if (current.visits.length >= HISTORY_MAX_LOADED) return;
    const owner = current.search;
    setLoadingMore(owner);
    browserHistoryPage({
      search: owner,
      before: current.next,
      limit: HISTORY_PAGE_SIZE,
    })
      .then((page) => {
        if (latest.current !== owner) return;
        setListing((loaded) =>
          loaded?.search === owner
            ? {
                ...loaded,
                visits: [...loaded.visits, ...page.visits].slice(
                  0,
                  HISTORY_MAX_LOADED,
                ),
                next: page.next,
              }
            : loaded,
        );
      })
      .catch((cause) => {
        if (latest.current === owner) setActionError(message(cause));
      })
      .finally(() =>
        setLoadingMore((loading) => (loading === owner ? null : loading)),
      );
  }, [busy, current]);

  const remove = useCallback((visit: BrowserVisit) => {
    setRemoving((ids) => new Set(ids).add(visit.id));
    setActionError(null);
    browserHistoryRemove([visit.id])
      .then(() =>
        setListing((loaded) =>
          loaded
            ? {
                ...loaded,
                visits: loaded.visits.filter((item) => item.id !== visit.id),
              }
            : loaded,
        ),
      )
      .catch((cause) => setActionError(message(cause)))
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
    loading: current === null,
    loadingMore: busy,
    canLoadMore,
    truncated: next !== null && !canLoadMore,
    error: actionError ?? current?.error ?? null,
    removing,
    loadMore,
    remove,
  };
}
