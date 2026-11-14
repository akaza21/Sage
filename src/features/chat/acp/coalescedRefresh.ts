export const REFRESH_DEBOUNCE_MS = 50;

export interface CoalescedRefresh {
  now(): void;
  schedule(beforeLoad?: () => void): void;
  dispose(): void;
}

// No client-side timeout: a hung read keeps holding its native page-read lease,
// so freeing the slot here would only turn the stall into capacity errors.
export interface CoalescedRefreshOptions {
  // A throw can start from the debounce timer or a rerun, where no caller would
  // see it.
  onThrow: (error: unknown) => void;
  maxInFlight?: number;
}

export function coalescedRefresh(
  load: () => Promise<unknown>,
  { onThrow, maxInFlight = 1 }: CoalescedRefreshOptions,
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
    const settle = () => {
      inFlight -= 1;
      if (rerun && id === newest) {
        rerun = false;
        now();
      }
    };
    let pending: Promise<unknown>;
    try {
      pending = load();
    } catch (error) {
      settle();
      onThrow(error);
      return;
    }
    void pending.then(settle, settle);
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
      }, REFRESH_DEBOUNCE_MS);
    },
    dispose() {
      disposed = true;
      if (timer) clearTimeout(timer);
      timer = null;
    },
  };
}
