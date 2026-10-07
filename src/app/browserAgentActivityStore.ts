import type { BrowserOperationProjection } from "../domain/browserOperation";
import { useEffect, useState } from "react";
import { create } from "zustand";

const STARTED_WINDOW_MS = 6 * 60_000;
const RECENT_WINDOW_MS = 5_000;
const MAX_TABS = 8;

interface BrowserAgentActivityState {
  byTab: Record<string, BrowserOperationProjection>;
  dismissedByTab: Record<string, string>;
  record: (operation: BrowserOperationProjection) => void;
  clear: (tabId: string) => void;
}

const useBrowserAgentActivityStore = create<BrowserAgentActivityState>(
  (set) => ({
    byTab: {},
    dismissedByTab: {},
    record: (operation) =>
      set((state) => {
        if (state.dismissedByTab[operation.tabId] === operation.operationId)
          return state;
        const current = state.byTab[operation.tabId];
        if (current && current.updatedAt > operation.updatedAt) return state;
        const entries = Object.entries({
          ...state.byTab,
          [operation.tabId]: operation,
        })
          .sort((left, right) => right[1].updatedAt - left[1].updatedAt)
          .slice(0, MAX_TABS);
        const dismissedByTab = { ...state.dismissedByTab };
        delete dismissedByTab[operation.tabId];
        return { byTab: Object.fromEntries(entries), dismissedByTab };
      }),
    clear: (tabId) =>
      set((state) => {
        if (!(tabId in state.byTab)) return state;
        const byTab = { ...state.byTab };
        const dismissedByTab = Object.fromEntries(
          [
            ...Object.entries(state.dismissedByTab).filter(
              ([key]) => key !== tabId,
            ),
            [tabId, byTab[tabId].operationId],
          ].slice(-MAX_TABS),
        );
        delete byTab[tabId];
        return { byTab, dismissedByTab };
      }),
  }),
);

export function recordBrowserAgentOperation(
  operation: BrowserOperationProjection,
): void {
  useBrowserAgentActivityStore.getState().record(operation);
}

export function clearBrowserAgentPresence(tabId: string): void {
  useBrowserAgentActivityStore.getState().clear(tabId);
}

export function useBrowserAgentPresence(
  projectId: string | null,
  tabId: string,
): BrowserOperationProjection | null {
  const operation = useBrowserAgentActivityStore(
    (state) => state.byTab[tabId] ?? null,
  );
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!operation) return;
    const windowMs =
      operation.phase === "started" ? STARTED_WINDOW_MS : RECENT_WINDOW_MS;
    const remaining = Math.max(
      0,
      Math.min(windowMs, operation.updatedAt + windowMs - Date.now()),
    );
    const timer = window.setTimeout(() => setNow(Date.now()), remaining + 1);
    return () => window.clearTimeout(timer);
  }, [operation]);
  if (!operation || !projectId || operation.projectId !== projectId)
    return null;
  const windowMs =
    operation.phase === "started" ? STARTED_WINDOW_MS : RECENT_WINDOW_MS;
  return now < operation.updatedAt + windowMs ? operation : null;
}
