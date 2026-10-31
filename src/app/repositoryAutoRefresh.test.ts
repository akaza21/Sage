import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceNotifyNotice } from "../features/files/workspaceNotifyCompose";
import {
  MAX_POLL_MS,
  MIN_POLL_MS,
  REFRESH_DEBOUNCE_MS,
  STAMP_INTERVAL_MS,
  pollDelay,
  startRepositoryAutoRefresh,
} from "./repositoryAutoRefresh";

let visibility: DocumentVisibilityState = "visible";
let notify: ((notice: WorkspaceNotifyNotice) => void) | null = null;
let refreshing: ((busy: boolean) => void) | null = null;
let stamp = "a";
let stop: (() => void) | null = null;

const refresh = vi.fn(() => Promise.resolve());
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
    subscribeRefreshing: (listener) => {
      refreshing = listener;
      return () => (refreshing = null);
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
  refresh.mockReset().mockImplementation(() => Promise.resolve());
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
      () => new Promise<void>((resolve) => (finish = resolve)),
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
    await advance(STAMP_INTERVAL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    await advance(STAMP_INTERVAL_MS * 5);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("keeps no timers and runs nothing while hidden", async () => {
    start(true);
    await advance(STAMP_INTERVAL_MS);
    setVisibility("hidden");
    await advance(0);
    expect(vi.getTimerCount()).toBe(0);
    const stamps = changeStamp.mock.calls.length;
    notice();
    stamp = "b";
    await advance(MAX_POLL_MS * 2);
    expect(refresh).not.toHaveBeenCalled();
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

  it("drops an ask that a refresh started after it already answered", async () => {
    start();
    notice();
    refreshing?.(true);
    refreshing?.(false);
    await advance(REFRESH_DEBOUNCE_MS * 2);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("keeps an ask that came in while another refresh ran", async () => {
    start();
    refreshing?.(true);
    await advance(1);
    notice();
    refreshing?.(false);
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("polls only where the watcher can miss edits, paced by Git", async () => {
    start(false);
    await advance(MAX_POLL_MS * 2);
    expect(refresh).not.toHaveBeenCalled();
    stop?.();
    start(true);
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(pollDelay(5)).toBe(MIN_POLL_MS);
    expect(pollDelay(600)).toBe(6_000);
    expect(pollDelay(5_000)).toBe(MAX_POLL_MS);
  });

  it("starts polling after the watcher overflows", async () => {
    start(false);
    notice("", "resync");
    await advance(REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
    await advance(MIN_POLL_MS + REFRESH_DEBOUNCE_MS);
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("stops every trigger when stopped", async () => {
    start(true);
    stop?.();
    stop = null;
    expect(vi.getTimerCount()).toBe(0);
    expect(notify).toBeNull();
    expect(refreshing).toBeNull();
    window.dispatchEvent(new Event("focus"));
    await advance(MAX_POLL_MS);
    expect(refresh).not.toHaveBeenCalled();
  });
});
