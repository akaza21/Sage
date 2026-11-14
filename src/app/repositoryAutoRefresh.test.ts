import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceNotifyNotice } from "../features/files/workspaceNotifyCompose";
import {
  MAX_IDLE_POLL_MS,
  MAX_POLL_MS,
  MAX_WAIT_MS,
  MIN_IDLE_POLL_MS,
  MIN_POLL_MS,
  REFRESH_DEBOUNCE_MS,
  RESYNC_POLL_MS,
  STAMP_INTERVAL_MS,
  pollDelay,
  startRepositoryAutoRefresh,
  type RefreshOutcome,
} from "./repositoryAutoRefresh";

let visibility: DocumentVisibilityState = "visible";
let notify: ((notice: WorkspaceNotifyNotice) => void) | null = null;
let stamp = "a";
let stop: (() => void) | null = null;

const refresh = vi.fn((): Promise<RefreshOutcome> =>
  Promise.resolve("changed"),
);
const changeStamp = vi.fn(() => Promise.resolve(stamp));

function notice(relativePath = "src/a.ts", kind = "modified") {
  notify?.({
    projectId: "p",
    projectGeneration: 1,
    transitionEpoch: 1,
    rootId: "r",
    relativePath,
    kind,
  });
}

function start(pollAlways = false) {
  stop = startRepositoryAutoRefresh({
    refresh,
    changeStamp,
    subscribeNotices: (listener) => {
      notify = listener;
      return () => (notify = null);
    },
    pollAlways,
  });
}

function setVisibility(next: DocumentVisibilityState) {
  visibility = next;
  document.dispatchEvent(new Event("visibilitychange"));
}

const advance = (ms: number) => vi.advanceTimersByTimeAsync(ms);

beforeEach(() => {
  vi.useFakeTimers();
  visibility = "visible";
  stamp = "a";
  refresh.mockReset().mockImplementation(() => Promise.resolve("changed"));
  changeStamp.mockClear();
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => visibility,
  });
});

afterEach(() => {
  stop?.();
  stop = null;
  vi.useRealTimers();
});

describe("repository auto refresh", () => {
  it("waits for changes to settle, then refreshes once", async () => {
    start();
    notice();
    await advance(REFRESH_DEBOUNCE_MS - 50);
    notice();
    await advance(REFRESH_DEBOUNCE_MS - 50);
    expect(refresh).not.toHaveBeenCalled();
    await advance(50);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("refreshes once for a storm of 1,000 changes", async () => {
    start();
    for (let index = 0; index < 1_000; index += 1) notice(`src/${index}.ts`);
    await advance(REFRESH_DEBOUNCE_MS * 4);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("runs one refresh at a time and one more for changes during it", async () => {
    let finish = () => undefined as void;
    refresh.mockImplementation(
      () =>
        new Promise<RefreshOutcome>(
          (resolve) => (finish = () => resolve("changed")),
        ),
    );
    start();
    notice();
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    for (let index = 0; index < 5; index += 1) {
      notice();
      await advance(REFRESH_DEBOUNCE_MS);
    }
    expect(refresh).toHaveBeenCalledTimes(1);
    finish();
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(2);
    finish();
    await advance(REFRESH_DEBOUNCE_MS * 4);
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("ignores Git's own files, which the stamp covers", async () => {
    start();
    notice(".git/objects/ab/cdef");
    notice(".git/index.lock");
    notice("packages/app/.git/HEAD");
    await advance(REFRESH_DEBOUNCE_MS * 2);
    expect(refresh).not.toHaveBeenCalled();
    notice(".github/workflows/ci.yml");
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("refreshes when the Git stamp changes, and not while it holds", async () => {
    start();
    await advance(STAMP_INTERVAL_MS * 5);
    expect(changeStamp.mock.calls.length).toBeGreaterThanOrEqual(5);
    expect(refresh).not.toHaveBeenCalled();
    stamp = "b";
    await advance(STAMP_INTERVAL_MS + 10);
    expect(refresh).toHaveBeenCalledTimes(1);
    await advance(STAMP_INTERVAL_MS * 5);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("keeps no timers and runs nothing while hidden", async () => {
    start();
    notice("", "resync");
    await advance(STAMP_INTERVAL_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    setVisibility("hidden");
    await advance(0);
    expect(vi.getTimerCount()).toBe(0);
    const stamps = changeStamp.mock.calls.length;
    notice();
    stamp = "b";
    await advance(MAX_POLL_MS * 2);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(changeStamp).toHaveBeenCalledTimes(stamps);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("refreshes once when shown and focused again", async () => {
    start();
    setVisibility("hidden");
    await advance(0);
    setVisibility("visible");
    window.dispatchEvent(new Event("focus"));
    await advance(REFRESH_DEBOUNCE_MS * 2);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("refreshes once on focus from another app while still visible", async () => {
    start();
    window.dispatchEvent(new Event("focus"));
    window.dispatchEvent(new Event("focus"));
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("refreshes within the longest wait while changes keep coming", async () => {
    start();
    for (let elapsed = 0; elapsed < MAX_WAIT_MS * 2; elapsed += 100) {
      notice(`target/${elapsed}.o`);
      await advance(100);
    }
    expect(refresh.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(refresh.mock.calls.length).toBeLessThanOrEqual(3);
  });

  it("tries again when the store was too busy to refresh", async () => {
    refresh.mockResolvedValueOnce("skipped");
    start();
    notice();
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    await advance(STAMP_INTERVAL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(2);
    await advance(STAMP_INTERVAL_MS * 3);
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("never polls Git while the watcher keeps up", async () => {
    start();
    await advance(MAX_POLL_MS * 4);
    expect(refresh).not.toHaveBeenCalled();
    expect(pollDelay(5)).toBe(MIN_POLL_MS);
    expect(pollDelay(600)).toBe(6_000);
    expect(pollDelay(5_000)).toBe(MAX_POLL_MS);
  });

  it("starts polling after the watcher overflows", async () => {
    start();
    notice("", "resync");
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("stops polling once the watcher has kept up for a while", async () => {
    start();
    notice("", "resync");
    await advance(RESYNC_POLL_MS + MAX_IDLE_POLL_MS);
    const polled = refresh.mock.calls.length;
    expect(polled).toBeGreaterThan(1);
    await advance(MAX_IDLE_POLL_MS * 4);
    expect(refresh).toHaveBeenCalledTimes(polled);
    notice("", "resync");
    await advance(REFRESH_DEBOUNCE_MS + MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(polled + 2);
  });

  it("polls from the start where the watcher misses edits", async () => {
    start(true);
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("backs off while polls find nothing, down to the idle cap", async () => {
    refresh.mockImplementation(() => Promise.resolve("unchanged"));
    start(true);
    const times: number[] = [];
    refresh.mockImplementation(() => {
      times.push(Date.now());
      return Promise.resolve("unchanged");
    });
    await advance(MIN_IDLE_POLL_MS * 8);
    const gaps = times.slice(1).map((time, index) => time - times[index]!);
    expect(gaps.slice(0, 4)).toEqual(
      [4_000, 8_000, 16_000, 30_000].map((wait) => wait + REFRESH_DEBOUNCE_MS),
    );
    expect(
      gaps
        .slice(4)
        .every((gap) => gap === MIN_IDLE_POLL_MS + REFRESH_DEBOUNCE_MS),
    ).toBe(true);
    expect(gaps.length).toBeGreaterThan(5);
  });

  it("polls fast again after any sign of activity", async () => {
    refresh.mockImplementation(() => Promise.resolve("unchanged"));
    start(true);
    await advance(MIN_IDLE_POLL_MS * 4);
    notice();
    await advance(REFRESH_DEBOUNCE_MS);
    const calls = refresh.mock.calls.length;
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(calls + 1);
  });

  it("polls fast again when a poll finds a change", async () => {
    const times: number[] = [];
    let changeAt = 6;
    refresh.mockImplementation(() => {
      times.push(Date.now());
      changeAt -= 1;
      return Promise.resolve(changeAt === 0 ? "changed" : "unchanged");
    });
    start(true);
    await advance(MIN_IDLE_POLL_MS * 4);
    const gaps = times.slice(1).map((time, index) => time - times[index]!);
    expect(gaps[4]).toBe(MIN_IDLE_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(gaps[5]).toBe(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
  });

  it("keeps the fast pace when a notice comes in during a poll", async () => {
    let finish = () => undefined as void;
    const times: number[] = [];
    refresh.mockImplementation(() => {
      times.push(Date.now());
      return new Promise<RefreshOutcome>(
        (resolve) => (finish = () => resolve("unchanged")),
      );
    });
    start(true);
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(times).toHaveLength(1);
    notice();
    finish();
    await advance(REFRESH_DEBOUNCE_MS);
    expect(times).toHaveLength(2);
    finish();
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(times).toHaveLength(3);
  });

  it("caps the idle poll so Git stays near 0.5% of one core", () => {
    expect(pollDelay(5, 3)).toBe(16_000);
    expect(pollDelay(5, 8)).toBe(MIN_IDLE_POLL_MS);
    expect(pollDelay(150, 8)).toBe(30_000);
    expect(pollDelay(400, 8)).toBe(80_000);
    expect(pollDelay(5_000, 8)).toBe(MAX_IDLE_POLL_MS);
    for (const statusMs of [5, 50, 150, 400, 600]) {
      expect(statusMs / pollDelay(statusMs, 8)).toBeLessThanOrEqual(0.005);
    }
  });

  it("stops every trigger when stopped", async () => {
    start();
    notice("", "resync");
    stop?.();
    stop = null;
    expect(vi.getTimerCount()).toBe(0);
    expect(notify).toBeNull();
    window.dispatchEvent(new Event("focus"));
    await advance(MAX_POLL_MS);
    expect(refresh).not.toHaveBeenCalled();
  });
});
