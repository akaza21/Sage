/** Pause after a change event before the page is read again, so a burst of
 * events during a streamed turn costs one read. */
export const PROJECTION_REFRESH_DEBOUNCE_MS = 50;

export interface CoalescedRefresh {
  /** Loads now, or once the newest load settles if too many are in flight. */
  now(): void;
  /** Loads after the debounce; `beforeLoad` runs when the pause ends. */
  schedule(beforeLoad?: () => void): void;
  dispose(): void;
}

/**
 * Keeps at most `maxInFlight` loads of one page running. A request beyond
 * that becomes a single rerun, started when the newest load settles, so a
 * burst of change events never stacks page reads (each Chat allows only a
 * few at once) and every rerun follows a load that completed. With one load
 * in flight, responses also land in request order. With two, a request can
 * still overtake a load that hangs (the caller discards the older answer),
 * and the newest load is superseded at most once before it lands.
 * `load` must settle; its own handlers report failures.
 */
export function coalescedRefresh(
  load: () => Promise<unknown>,
  maxInFlight = 1,
): CoalescedRefresh {
  let disposed = false;
  let inFlight = 0;
  let newest = 0;
  let rerun = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const now = () => {
    if (disposed) return;
    if (inFlight >= maxInFlight) {
      rerun = true;
      return;
    }
    inFlight += 1;
    const id = ++newest;
    void load().finally(() => {
      inFlight -= 1;
      if (rerun && id === newest) {
        rerun = false;
        now();
      }
    });
  };

  return {
    now,
    schedule(beforeLoad) {
      if (disposed || timer) return;
      timer = setTimeout(() => {
        timer = null;
        if (disposed) return;
        beforeLoad?.();
        now();
      }, PROJECTION_REFRESH_DEBOUNCE_MS);
    },
    dispose() {
      disposed = true;
      if (timer) clearTimeout(timer);
      timer = null;
    },
  };
}
