import type { BrowserOperationProjection } from "../domain/browserOperation";
import { useEffect, useState } from "react";
import { create } from "zustand";

const STARTED_WINDOW_MS = 6 * 60_000;
const RECENT_WINDOW_MS = 5_000;
const MAX_TABS = 8;

interface BrowserAgentActivityState {
  byTab: Record<string, BrowserOperationProjection>;
  dismissedByTab: Record<string, string>;
  /** Tabs the user took over, by the chat whose Agent waits. Rust gates. */
  userControlByTab: Record<string, string>;
  record: (operation: BrowserOperationProjection) => void;
  clear: (tabId: string) => void;
  setUserControl: (tabId: string, chatId: string | null) => void;
}

const useBrowserAgentActivityStore = create<BrowserAgentActivityState>(
  (set) => ({
    byTab: {},
    dismissedByTab: {},
    userControlByTab: {},
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
        const userControlByTab = withoutTab(state.userControlByTab, tabId);
        if (!(tabId in state.byTab))
          return userControlByTab === state.userControlByTab
            ? state
            : { userControlByTab };
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
        return { byTab, dismissedByTab, userControlByTab };
      }),
    setUserControl: (tabId, chatId) =>
      set((state) => ({
        userControlByTab: chatId
          ? Object.fromEntries(
              [
                ...Object.entries(state.userControlByTab).filter(
                  ([key]) => key !== tabId,
                ),
                [tabId, chatId],
              ].slice(-MAX_TABS),
            )
          : withoutTab(state.userControlByTab, tabId),
      })),
  }),
);

function withoutTab(
  entries: Record<string, string>,
  tabId: string,
): Record<string, string> {
  if (!(tabId in entries)) return entries;
  const next = { ...entries };
  delete next[tabId];
  return next;
}

export function recordBrowserAgentOperation(
  operation: BrowserOperationProjection,
): void {
  useBrowserAgentActivityStore.getState().record(operation);
}

export function clearBrowserAgentPresence(tabId: string): void {
  useBrowserAgentActivityStore.getState().clear(tabId);
}

/** Mirrors Rust after a successful take over (chatId) or hand back (null). */
export function setBrowserUserControl(
  tabId: string,
  chatId: string | null,
): void {
  useBrowserAgentActivityStore.getState().setUserControl(tabId, chatId);
}

export function useBrowserUserControl(tabId: string): string | null {
  return useBrowserAgentActivityStore(
    (state) => state.userControlByTab[tabId] ?? null,
  );
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
