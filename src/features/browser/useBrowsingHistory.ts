import { useCallback, useEffect, useRef, useState } from "react";
import {
  browserHistoryPage,
  browserHistoryRemove,
  type BrowserHistoryCursor,
  type BrowserVisit,
} from "../../ports/nativeBrowserHistoryPort";

export const HISTORY_PAGE_SIZE = 50;
/** Older visits need a narrower search. */
export const HISTORY_MAX_LOADED = 500;
export const HISTORY_SEARCH_DELAY_MS = 200;

export interface BrowsingHistoryView {
  visits: readonly BrowserVisit[];
  loading: boolean;
  loadingMore: boolean;
  canLoadMore: boolean;
  /** Another page exists but the loaded list reached HISTORY_MAX_LOADED. */
  truncated: boolean;
  error: string | null;
  removing: ReadonlySet<number>;
  loadMore: () => void;
  remove: (visit: BrowserVisit) => void;
}

interface Listing {
  search: string;
  /** Bumped on every fresh first page, so a page fetched for an earlier
   * listing is never appended to a newer one. */
  generation: number;
  visits: BrowserVisit[];
  next: BrowserHistoryCursor | null;
  error: string | null;
}

function message(cause: unknown): string {
  return String(cause).slice(0, 512);
}

export function useBrowsingHistory(search: string): BrowsingHistoryView {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loadingMore, setLoadingMore] = useState<number | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ReadonlySet<number>>(new Set());
  const generation = useRef(0);

  useEffect(() => {
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
            generation.current += 1;
            setListing({ ...answer, generation: generation.current });
            setActionError(null);
          }
        });
    };
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
  const busy = current !== null && loadingMore === current.generation;

  const loadMore = useCallback(() => {
    if (!current || !current.next || busy) return;
    // Only what still fits, so the cursor never skips visits not kept.
    const room = HISTORY_MAX_LOADED - current.visits.length;
    if (room <= 0) return;
    const owner = current.generation;
    setLoadingMore(owner);
    browserHistoryPage({
      search: current.search,
      before: current.next,
      limit: Math.min(HISTORY_PAGE_SIZE, room),
    })
      .then((page) =>
        setListing((loaded) =>
          loaded?.generation === owner
            ? {
                ...loaded,
                visits: [...loaded.visits, ...page.visits],
                next: page.next,
              }
            : loaded,
        ),
      )
      .catch((cause) => {
        if (generation.current === owner) setActionError(message(cause));
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
