import { useEffect } from "react";

import { useWorkbenchStore } from "../../app/store";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { whenIdle } from "../../ui/whenIdle";

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
const preloadWhenIdle = () =>
  whenIdle(() => void loadQuickOpen().catch(() => undefined));

/** Mounts Quick Open while it is open and keeps its recent Files. */
export function QuickOpenHost() {
  const open = useQuickOpenStore((state) => state.open);
  const opening = useQuickOpenStore((state) => state.opening);
  const seed = useQuickOpenStore((state) => state.seed);
  useEffect(preloadWhenIdle, []);
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
      render={(QuickOpen) => (
        <QuickOpen key={opening} seed={seed} onClose={closeQuickOpen} />
      )}
    />
  );
}

export default QuickOpenHost;
