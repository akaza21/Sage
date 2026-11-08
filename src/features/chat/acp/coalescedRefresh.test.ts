import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  coalescedRefresh,
  PROJECTION_REFRESH_DEBOUNCE_MS,
} from "./coalescedRefresh";

function deferredLoads() {
  const settles: Array<() => void> = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const load = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        inFlight += 1;
        maxInFlight = Math.max(maxInFlight, inFlight);
        settles.push(() => {
          inFlight -= 1;
          resolve();
        });
      }),
  );
  return {
    load,
    settleNext: async () => {
      settles.shift()?.();
      await Promise.resolve();
      await Promise.resolve();
    },
    maxInFlight: () => maxInFlight,
  };
}

describe("coalescedRefresh", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("folds a burst of scheduled refreshes into one load after the pause", () => {
    const loads = deferredLoads();
    const beforeLoad = vi.fn();
    const refresh = coalescedRefresh(loads.load);

    for (let event = 0; event < 20; event += 1) refresh.schedule(beforeLoad);
    vi.advanceTimersByTime(PROJECTION_REFRESH_DEBOUNCE_MS - 1);
    expect(loads.load).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);

    expect(beforeLoad).toHaveBeenCalledTimes(1);
    expect(loads.load).toHaveBeenCalledTimes(1);
  });

  it("never overlaps loads and reruns once for requests made during one", async () => {
    const loads = deferredLoads();
    const refresh = coalescedRefresh(loads.load);

    refresh.now();
    for (let event = 0; event < 5; event += 1) {
      refresh.schedule();
      vi.advanceTimersByTime(PROJECTION_REFRESH_DEBOUNCE_MS);
      refresh.now();
    }
    expect(loads.load).toHaveBeenCalledTimes(1);

    await loads.settleNext();
    expect(loads.load).toHaveBeenCalledTimes(2);
    await loads.settleNext();
    expect(loads.load).toHaveBeenCalledTimes(2);
    expect(loads.maxInFlight()).toBe(1);
  });

  it("starts nothing once disposed, including a rerun owed by a load in flight", async () => {
    const loads = deferredLoads();
    const refresh = coalescedRefresh(loads.load);

    refresh.now();
    refresh.now();
    refresh.schedule();
    refresh.dispose();
    vi.advanceTimersByTime(PROJECTION_REFRESH_DEBOUNCE_MS);
    await loads.settleNext();

    expect(loads.load).toHaveBeenCalledTimes(1);
  });

  it("with two in flight, overtakes a hung load once and reruns only after the newest lands", async () => {
    const loads = deferredLoads();
    const refresh = coalescedRefresh(loads.load, 2);

    refresh.now(); // hangs
    refresh.now(); // overtakes it
    refresh.now();
    refresh.now();
    expect(loads.load).toHaveBeenCalledTimes(2);
    expect(loads.maxInFlight()).toBe(2);

    await loads.settleNext(); // the hung load ends: not the newest, no rerun
    expect(loads.load).toHaveBeenCalledTimes(2);
    await loads.settleNext(); // the newest lands: one rerun for both requests
    expect(loads.load).toHaveBeenCalledTimes(3);
  });
});
