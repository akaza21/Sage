/**
 * The longest an idle preload waits. WebKit (the macOS and Linux webview)
 * has no requestIdleCallback, so there the preload waits this long instead
 * of running during startup.
 */
export const IDLE_PRELOAD_TIMEOUT_MS = 2000;

/** Runs `run` once the window is idle; returns a cancel. */
export function whenIdle(run: () => void): () => void {
  if (typeof window.requestIdleCallback === "function") {
    const handle = window.requestIdleCallback(run, {
      timeout: IDLE_PRELOAD_TIMEOUT_MS,
    });
    return () => window.cancelIdleCallback(handle);
  }
  const timer = window.setTimeout(run, IDLE_PRELOAD_TIMEOUT_MS);
  return () => window.clearTimeout(timer);
}
