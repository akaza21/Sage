// Replace loads when the user opens it, or earlier when they reach for its
// toggle or the window is idle, so Search alone carries none of it.

import type * as ReplaceParts from "./WorkspaceReplaceParts";

let replaceParts: typeof ReplaceParts | undefined;

/** The Replace parts, if they have loaded. */
export const loadedReplaceParts = () => replaceParts;

export const loadReplace = () =>
  import("./WorkspaceReplaceParts").then((parts) => (replaceParts = parts));

export const preloadReplace = () => void loadReplace().catch(() => undefined);

export function preloadReplaceWhenIdle() {
  if (typeof window.requestIdleCallback === "function") {
    const handle = window.requestIdleCallback(preloadReplace, {
      timeout: 2000,
    });
    return () => window.cancelIdleCallback(handle);
  }
  const timer = setTimeout(preloadReplace, 2000);
  return () => clearTimeout(timer);
}
