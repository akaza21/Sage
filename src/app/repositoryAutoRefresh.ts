// Keeps Git state current without a click (EP-12), as VS Code does. Three
// triggers ask for a refresh: a workspace change notice, a changed stamp of
// the Git files a commit, checkout or stage touches (checked every second),
// and, where the watcher can miss edits, a slow poll paced by how long Git
// status takes. They share one debounce and one refresh at a time. While the
// window is hidden nothing runs; showing or focusing it refreshes once.

import type { WorkspaceNotifyNotice } from "../features/files/workspaceNotifyCompose";

export const REFRESH_DEBOUNCE_MS = 300;
export const STAMP_INTERVAL_MS = 1_000;
export const MIN_POLL_MS = 2_000;
export const MAX_POLL_MS = 15_000;
const POLL_STATUS_FACTOR = 10;

export interface RepositoryAutoRefreshDeps {
  refresh(): Promise<void>;
  changeStamp(): Promise<string>;
  subscribeNotices(
    listener: (notice: WorkspaceNotifyNotice) => void,
  ): () => void;
  /** Reports when a refresh anyone asked for starts and ends. */
  subscribeRefreshing(listener: (refreshing: boolean) => void): () => void;
  /** The watcher sees only some edits here (macOS watches folders). */
  pollAlways: boolean;
}

/** Git rewrites these by the hundred in a rebase; the stamp covers the rest. */
const isGitInternal = (path: string) => /(?:^|\/)\.git(?:\/|$)/i.test(path);

export const pollDelay = (statusMs: number) =>
  Math.min(MAX_POLL_MS, Math.max(MIN_POLL_MS, statusMs * POLL_STATUS_FACTOR));

/** Starts the triggers; the returned function stops them all. */
export function startRepositoryAutoRefresh(
  deps: RepositoryAutoRefreshDeps,
): () => void {
  let stopped = false;
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let stampTimer: ReturnType<typeof setTimeout> | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let refreshing = false;
  let rerun = false;
  let stampInFlight = false;
  let lastStamp: string | null = null;
  let polling = deps.pollAlways;
  // How long the last refresh took, which paces the poll.
  let lastRefreshMs = 0;
  // When the oldest unanswered ask came in, and when a refresh someone else
  // started began: a refresh that began after the ask has answered it.
  let askedAt: number | null = null;
  let otherStartedAt: number | null = null;

  const visible = () => document.visibilityState === "visible";

  const clear = (timer: ReturnType<typeof setTimeout> | null) => {
    if (timer !== null) clearTimeout(timer);
    return null;
  };

  const request = () => {
    if (stopped || !visible()) return;
    askedAt ??= performance.now();
    debounce = clear(debounce);
    debounce = setTimeout(() => void flush(), REFRESH_DEBOUNCE_MS);
  };

  const flush = async () => {
    debounce = null;
    if (refreshing) {
      rerun = true;
      return;
    }
    refreshing = true;
    askedAt = null;
    const started = performance.now();
    try {
      await deps.refresh();
    } catch {
      /* the store reports refresh errors */
    }
    lastRefreshMs = performance.now() - started;
    refreshing = false;
    if (stopped) return;
    if (rerun) {
      rerun = false;
      request();
    }
    schedulePoll();
  };

  const schedulePoll = () => {
    pollTimer = clear(pollTimer);
    if (stopped || !polling || !visible()) return;
    pollTimer = setTimeout(request, pollDelay(lastRefreshMs));
  };

  const checkStamp = async () => {
    stampTimer = null;
    if (stopped || !visible()) return;
    if (!stampInFlight) {
      stampInFlight = true;
      try {
        const stamp = await deps.changeStamp();
        if (lastStamp !== null && stamp !== lastStamp) request();
        lastStamp = stamp;
      } catch {
        /* no Project, or it is changing: try again next tick */
      }
      stampInFlight = false;
    }
    if (!stopped && visible() && stampTimer === null) {
      stampTimer = setTimeout(() => void checkStamp(), STAMP_INTERVAL_MS);
    }
  };

  const pause = () => {
    debounce = clear(debounce);
    stampTimer = clear(stampTimer);
    pollTimer = clear(pollTimer);
    rerun = false;
    askedAt = null;
  };

  const resume = () => {
    request();
    if (stampTimer === null) void checkStamp();
    schedulePoll();
  };

  const onVisibility = () => (visible() ? resume() : pause());
  const onFocus = () => request();

  const stopNotices = deps.subscribeNotices((notice) => {
    // An overflow means the watcher lost events: poll from now on.
    if (notice.kind === "resync" && !polling) {
      polling = true;
      schedulePoll();
    }
    if (!isGitInternal(notice.relativePath)) request();
  });
  const stopRefreshing = deps.subscribeRefreshing((busy) => {
    if (refreshing) return;
    if (busy) {
      otherStartedAt = performance.now();
    } else if (
      debounce !== null &&
      askedAt !== null &&
      otherStartedAt !== null &&
      askedAt <= otherStartedAt
    ) {
      // A save or a Source Control action refreshed after the ask came in.
      debounce = clear(debounce);
      askedAt = null;
      schedulePoll();
    }
  });

  document.addEventListener("visibilitychange", onVisibility);
  window.addEventListener("focus", onFocus);
  if (visible()) {
    void checkStamp();
    schedulePoll();
  }

  return () => {
    stopped = true;
    pause();
    stopNotices();
    stopRefreshing();
    document.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("focus", onFocus);
  };
}
