import type { StateCreator } from "zustand";

import { browserTabTitle } from "../domain/browserTabTitle";

import type { BrowserOperationProjection } from "../domain/browserOperation";
import { browserFaviconDataUrl } from "../features/browser/browserFavicon";
import {
  browserPopupAbandon,
  browserPopupPending,
  closeBrowser,
  openBrowser,
  type BrowserNavigation,
  type BrowserPopupIntent,
} from "../ports/nativeBrowserPort";
import { isTauriRuntime } from "../ports/nativeInvoke";
import {
  clearBrowserAgentPresence,
  recordBrowserAgentOperation,
} from "./browserAgentActivityStore";
import { matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";
import {
  BROWSER_CLOSE_ERROR,
  consumeBrowserAlreadyClosed,
  isBrowserCloseInFlight,
  reconcileNativeClosedBrowserTabs,
} from "./tabClosing";

export interface BrowserSlice {
  userOpenedBrowserTabId: string | null;
  openBrowser: (url?: string) => Promise<void>;
  updateBrowserTabTitle: (
    tabId: string,
    url: string,
    documentTitle: string | null | undefined,
    revision: string,
  ) => void;
  updateBrowserTabFavicon: (
    tabId: string,
    dataUrl: string | null | undefined,
    revision: string,
  ) => void;
}

export const createBrowserSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  BrowserSlice
> = (set, get, store) => {
  const titleRevisions = new Map<string, bigint>();
  const faviconRevisions = new Map<string, bigint>();
  const nativeRuntime = isTauriRuntime();
  const processingPopups = new Set<number>();
  const replayAfterProcessing = new Set<number>();
  const finishedPopups = new Set<number>();
  async function processPopup(intent: BrowserPopupIntent) {
    if (
      !Number.isSafeInteger(intent.id) ||
      intent.id < 1 ||
      typeof intent.openerTabId !== "string" ||
      intent.openerTabId.length > 64 ||
      finishedPopups.has(intent.id)
    )
      return;
    if (processingPopups.has(intent.id)) {
      replayAfterProcessing.add(intent.id);
      return;
    }
    processingPopups.add(intent.id);
    try {
      const { adoptPopup } = await import("./browserPopupNative");
      if (!(await adoptPopup(set, get, intent))) return;
      finishedPopups.add(intent.id);
      if (finishedPopups.size > 32) {
        const first = finishedPopups.values().next().value;
        if (first !== undefined) finishedPopups.delete(first);
      }
    } catch {
      if (typeof intent.nativeTabId === "string")
        await browserPopupAbandon(
          intent.id,
          intent.openerTabId,
          intent.nativeTabId,
        ).catch(() => undefined);
      set({ error: "Could not open the browser popup. Try again." });
    } finally {
      processingPopups.delete(intent.id);
      if (replayAfterProcessing.delete(intent.id)) replayPopups();
    }
  }
  function replayPopups() {
    void browserPopupPending()
      .then((pending) => {
        for (const intent of pending) void processPopup(intent);
      })
      .catch(() =>
        set({ error: "Could not restore browser popups. Try again." }),
      );
  }
  if (nativeRuntime) {
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const removers: Array<() => void> = [];
        let disposed = false;
        const dispose = () => {
          disposed = true;
          for (const remove of removers) remove();
          window.removeEventListener("beforeunload", dispose);
        };
        const keep = (remove: () => void) => {
          if (disposed) remove();
          else removers.push(remove);
        };
        window.addEventListener("beforeunload", dispose, { once: true });
        try {
          keep(
            await listen<BrowserPopupIntent>(
              "browser:popup-request",
              (event) => {
                void processPopup(event.payload);
              },
            ),
          );
          replayPopups();
          keep(
            await listen<BrowserOperationProjection>(
              "browser:agent-operation",
              (event) => {
                recordBrowserAgentOperation(event.payload);
              },
            ),
          );
          keep(
            await listen<string>("browser:tab-closed", (event) => {
              reconcileNativeClosedBrowserTabs(set, get, [event.payload]);
            }),
          );
          keep(
            await listen<BrowserNavigation>("browser:navigation", (event) => {
              const status = event.payload;
              get().updateBrowserTabFavicon(
                status.tabId,
                status.favicon,
                status.revision,
              );
              if (!status.loading && !status.error && status.url)
                get().updateBrowserTabTitle(
                  status.tabId,
                  status.url,
                  status.title,
                  status.revision,
                );
            }),
          );
          keep(
            await listen<{ projectId: string; tabId: string; url: string }>(
              "browser:agent-opened",
              (event) => {
                const { projectId, tabId, url } = event.payload;
                const generation = get().projectGeneration;
                if (!matchesProjectIntent(get(), generation, projectId)) return;
                if (get().tabs.some((tab) => tab.id === tabId)) return;
                let flush: Promise<void>;
                try {
                  flush = get().flushActiveEditor();
                } catch (error) {
                  flush = Promise.reject(error);
                }
                let title = "New tab";
                try {
                  title = new URL(url).host.slice(0, 48);
                } catch {
                  // Retain the default title when the native URL is unreadable.
                }
                set((state) => ({
                  tabs: [
                    ...state.tabs,
                    {
                      id: tabId,
                      kind: "browser",
                      title,
                      position: state.tabs.length,
                      pinned: false,
                      dirty: false,
                    },
                  ],
                  activeTabId: tabId,
                  error: null,
                }));
                void flush
                  .then(() => {
                    if (
                      matchesProjectIntent(get(), generation, projectId) &&
                      get().activeTabId === tabId
                    )
                      set({ activeEditor: null });
                  })
                  .catch(() =>
                    set({ error: "Could not save the editor. Try again." }),
                  );
              },
            ),
          );
        } catch {
          dispose();
        }
      })
      .catch(() => undefined);
  }
  store.subscribe((state, previous) => {
    if (
      nativeRuntime &&
      state.project &&
      !state.projectTransitioning &&
      (previous.project?.id !== state.project.id ||
        previous.projectTransitioning)
    )
      replayPopups();
    if (state.tabs === previous.tabs) return;
    const remaining = new Set(
      state.tabs.filter((tab) => tab.kind === "browser").map((tab) => tab.id),
    );
    for (const tab of previous.tabs) {
      if (tab.kind === "browser" && !remaining.has(tab.id)) {
        if (tab.status === "closed") {
          titleRevisions.delete(tab.id);
          faviconRevisions.delete(tab.id);
          clearBrowserAgentPresence(tab.id);
          continue;
        }
        if (
          consumeBrowserAlreadyClosed(tab.id) ||
          isBrowserCloseInFlight(tab.id)
        ) {
          titleRevisions.delete(tab.id);
          faviconRevisions.delete(tab.id);
          clearBrowserAgentPresence(tab.id);
          continue;
        }
        void closeBrowser(tab.id)
          .then(() => {
            titleRevisions.delete(tab.id);
            faviconRevisions.delete(tab.id);
            clearBrowserAgentPresence(tab.id);
          })
          .catch(() => {
            set((current) => ({
              tabs: current.tabs.some((item) => item.id === tab.id)
                ? current.tabs
                : [...current.tabs, tab],
              error: BROWSER_CLOSE_ERROR,
            }));
          });
      }
    }
  });
  return {
    userOpenedBrowserTabId: null,
    updateBrowserTabFavicon: (tabId, dataUrl, revision) => {
      if (!/^(0|[1-9]\d{0,19})$/.test(revision)) return;
      const incoming = BigInt(revision);
      if (incoming <= (faviconRevisions.get(tabId) ?? -1n)) return;
      if (!get().tabs.some((tab) => tab.id === tabId && tab.kind === "browser"))
        return;
      faviconRevisions.set(tabId, incoming);
      const favicon = browserFaviconDataUrl(dataUrl) ?? undefined;
      set((state) => ({
        tabs: state.tabs.map((tab) =>
          tab.id === tabId && tab.kind === "browser" && tab.favicon !== favicon
            ? { ...tab, favicon }
            : tab,
        ),
      }));
    },
    updateBrowserTabTitle: (tabId, url, documentTitle, revision) => {
      if (!get().tabs.some((tab) => tab.id === tabId && tab.kind === "browser"))
        return;
      if (!/^(0|[1-9]\d{0,19})$/.test(revision)) return;
      const incoming = BigInt(revision);
      if (incoming <= (titleRevisions.get(tabId) ?? -1n)) return;
      titleRevisions.set(tabId, incoming);
      const title = browserTabTitle(url, documentTitle);
      if (!title) return;
      set((state) => ({
        tabs: state.tabs.map((tab) =>
          tab.id === tabId && tab.kind === "browser" && tab.title !== title
            ? { ...tab, title }
            : tab,
        ),
      }));
    },
    openBrowser: async (url) => {
      const projectId = get().project?.id;
      if (!projectId || get().projectTransitioning) return;
      if (!isTauriRuntime())
        return set({
          error: "The in-app browser is available in the desktop app.",
        });
      const generation = get().projectGeneration;
      try {
        await get().flushActiveEditor();
        if (!matchesProjectIntent(get(), generation, projectId)) return;
        const id = await openBrowser(url);
        if (!matchesProjectIntent(get(), generation, projectId)) {
          await closeBrowser(id);
          return;
        }
        set((state) => ({
          tabs: [
            ...state.tabs,
            {
              id,
              kind: "browser",
              title: url ? (browserTabTitle(url) ?? "New tab") : "New tab",
              position: state.tabs.length,
              pinned: false,
              dirty: false,
            },
          ],
          activeTabId: id,
          userOpenedBrowserTabId: id,
          activeEditor: null,
          error: null,
        }));
      } catch {
        if (matchesProjectIntent(get(), generation, projectId))
          set({ error: "Could not open the browser tab. Try again." });
      }
    },
  };
};
