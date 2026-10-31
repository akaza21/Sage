// Keeps Git state current without a click (EP-12), as VS Code does. Three
// triggers ask for a refresh: a workspace change notice, a changed stamp of
// the Git files a commit, checkout or stage touches (checked every 500 ms),
// and, where the watcher cannot see every edit, a poll of Git. They share one
// debounce, which waits at most MAX_WAIT_MS so a busy build cannot hold a
// refresh back, and one refresh at a time. While the window is hidden nothing
// runs; showing or focusing it refreshes once.
//
// The poll backs off: each poll that finds nothing new doubles the wait, up
// to a cap that keeps an idle window's Git work near 0.5% of one core, and
// any sign of activity (a notice, focus, a Git operation, a poll that found a
// change) brings it back to its fastest.

import type { WorkspaceNotifyNotice } from "../features/files/workspaceNotifyCompose";

export const REFRESH_DEBOUNCE_MS = 300;
export const MAX_WAIT_MS = 1_500;
export const STAMP_INTERVAL_MS = 500;
export const MIN_POLL_MS = 2_000;
export const MAX_POLL_MS = 15_000;
export const MIN_IDLE_POLL_MS = 30_000;
export const MAX_IDLE_POLL_MS = 120_000;
const POLL_STATUS_FACTOR = 10;
const IDLE_STATUS_FACTOR = 200;
const MAX_BACKOFF_STEPS = 8;

/** `skipped`: the store was busy and refreshed nothing. */
export type RefreshOutcome = "changed" | "unchanged" | "skipped";

export interface RepositoryAutoRefreshDeps {
  refresh(): Promise<RefreshOutcome>;
  changeStamp(): Promise<string>;
  subscribeNotices(
    listener: (notice: WorkspaceNotifyNotice) => void,
  ): () => void;
  /** The watcher misses some edits from the start (see the hook). */
  pollAlways: boolean;
}

/** Git rewrites these by the hundred in a rebase; the stamp covers the rest. */
const isGitInternal = (path: string) => /(?:^|\/)\.git(?:\/|$)/i.test(path);

const clamp = (low: number, high: number, value: number) =>
  Math.min(high, Math.max(low, value));

/** The wait before a poll, after `idlePolls` polls in a row found nothing. */
export const pollDelay = (statusMs: number, idlePolls = 0) =>
  Math.min(
    clamp(MIN_IDLE_POLL_MS, MAX_IDLE_POLL_MS, statusMs * IDLE_STATUS_FACTOR),
    clamp(MIN_POLL_MS, MAX_POLL_MS, statusMs * POLL_STATUS_FACTOR) *
      2 ** idlePolls,
  );

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
  let idlePolls = 0;
  // Whether anything but the poll asked for the pending refresh.
  let activity = false;
  // How long the last refresh took, which paces the poll.
  let lastRefreshMs = 0;
  // When the oldest unanswered ask came in, which bounds the debounce.
  let askedAt: number | null = null;

  const visible = () => document.visibilityState === "visible";

  const clear = (timer: ReturnType<typeof setTimeout> | null) => {
    if (timer !== null) clearTimeout(timer);
    return null;
  };

  // A stamp change comes at most once a tick, so it need not settle first.
  const request = (settle = REFRESH_DEBOUNCE_MS, fromPoll = false) => {
    if (stopped || !visible()) return;
    if (!fromPoll) {
      activity = true;
      idlePolls = 0;
    }
    const now = performance.now();
    askedAt ??= now;
    debounce = clear(debounce);
    const wait = Math.min(settle, askedAt + MAX_WAIT_MS - now);
    debounce = setTimeout(() => void flush(), Math.max(0, wait));
  };

  const flush = async () => {
    debounce = null;
    if (refreshing) {
      rerun = true;
      return;
    }
    refreshing = true;
    askedAt = null;
    const pollOnly = !activity;
    activity = false;
    const started = performance.now();
    let outcome: RefreshOutcome = "changed";
    try {
      outcome = await deps.refresh();
    } catch {
      /* the store reports refresh errors */
    }
    lastRefreshMs = performance.now() - started;
    refreshing = false;
    if (stopped) return;
    if (outcome === "changed") idlePolls = 0;
    // A notice that came in while it ran keeps the poll at its fastest.
    else if (outcome === "unchanged" && pollOnly && !activity) {
      idlePolls = Math.min(MAX_BACKOFF_STEPS, idlePolls + 1);
    }
    if (outcome === "skipped") {
      // A rename, commit or Project change kept it from running: try again
      // in a moment, when the store is likely free.
      pollTimer = clear(pollTimer);
      pollTimer = setTimeout(() => request(), STAMP_INTERVAL_MS);
      return;
    }
    if (rerun) {
      rerun = false;
      request();
    }
    schedulePoll();
  };

  const schedulePoll = () => {
    pollTimer = clear(pollTimer);
    if (stopped || !polling || !visible()) return;
    pollTimer = setTimeout(
      () => request(REFRESH_DEBOUNCE_MS, true),
      pollDelay(lastRefreshMs, idlePolls),
    );
  };

  const checkStamp = async () => {
    stampTimer = null;
    if (stopped || !visible()) return;
    if (!stampInFlight) {
      stampInFlight = true;
      try {
        const stamp = await deps.changeStamp();
        if (lastStamp !== null && stamp !== lastStamp) request(0);
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
    // The watcher lost events, or cannot say which file changed: from now on
    // the poll covers what it misses.
    if (notice.kind === "resync" && !polling) {
      polling = true;
      schedulePoll();
    }
    if (!isGitInternal(notice.relativePath)) request();
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
    document.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("focus", onFocus);
  };
}
