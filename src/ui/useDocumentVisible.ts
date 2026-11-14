import { useSyncExternalStore } from "react";

function subscribe(notify: () => void) {
  document.addEventListener("visibilitychange", notify);
  return () => document.removeEventListener("visibilitychange", notify);
}

export function useDocumentVisible() {
  return useSyncExternalStore(
    subscribe,
    () => document.visibilityState !== "hidden",
  );
}
