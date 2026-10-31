import { useEffect } from "react";

import { useWorkbenchStore } from "../../app/store";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

import {
  closeQuickOpen,
  noteRecentFile,
  useQuickOpenStore,
} from "./quickOpenState";

const loadQuickOpen = () => import("./QuickOpen");
/**
 * Fetches the chunk once the window is idle. The first open then only waits
 * a microtask for the already-loaded module, not for the network or disk.
 */
const PRELOAD_TIMEOUT_MS = 2000;

function preloadWhenIdle(): () => void {
  const preload = () => void loadQuickOpen().catch(() => undefined);
  if (typeof window.requestIdleCallback === "function") {
    const handle = window.requestIdleCallback(preload, {
      timeout: PRELOAD_TIMEOUT_MS,
    });
    return () => window.cancelIdleCallback(handle);
  }
  const timer = setTimeout(preload, PRELOAD_TIMEOUT_MS);
  return () => clearTimeout(timer);
}

/** Mounts Quick Open while it is open and keeps its recent Files. */
export function QuickOpenHost() {
  const open = useQuickOpenStore((state) => state.open);
  useEffect(preloadWhenIdle, []);
  // Leaving the editor closes it, so it doesn't reopen on the way back.
  useEffect(() => closeQuickOpen, []);
  useEffect(() => {
    const note = (state: ReturnType<typeof useWorkbenchStore.getState>) => {
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      if (state.project && tab?.kind === "file" && tab.resource) {
        noteRecentFile(state.project.id, tab.resource);
      }
    };
    note(useWorkbenchStore.getState());
    return useWorkbenchStore.subscribe((state, previous) => {
      if (state.activeTabId !== previous.activeTabId) note(state);
    });
  }, []);
  if (!open) return null;
  return (
    <LazyModuleBoundary
      label="Quick Open"
      load={loadQuickOpen}
      fallback={null}
      render={(QuickOpen) => <QuickOpen onClose={closeQuickOpen} />}
    />
  );
}
