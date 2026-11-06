import { useSyncExternalStore } from "react";

function subscribe(notify: () => void) {
  document.addEventListener("visibilitychange", notify);
  return () => document.removeEventListener("visibilitychange", notify);
}

/** Minimized windows sleep their rendering while resource owners stay alive. */
export function useDocumentVisible() {
  return useSyncExternalStore(
    subscribe,
    () => document.visibilityState !== "hidden",
  );
}
