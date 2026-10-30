import type { StateCreator } from "zustand";

import {
  closeBrowser,
  openBrowser,
  browserPopupPending,
  browserOpenPopup,
  type BrowserPopupIntent,
  type BrowserNavigation,
} from "../ports/nativeBrowserPort";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { browserFaviconDataUrl } from "../features/browser/browserFavicon";
import {
  clearBrowserAgentPresence,
  recordBrowserAgentOperation,
} from "./browserAgentActivityStore";
import type { BrowserOperationProjection } from "../domain/browserOperation";
import { matchesProjectIntent } from "./projectIntent";
import {
  BROWSER_CLOSE_ERROR,
  consumeBrowserAlreadyClosed,
  isBrowserCloseInFlight,
  reconcileNativeClosedBrowserTabs,
} from "./tabClosing";
import type { WorkbenchState } from "./store";
import {
  abandonNativePopup,
  abandonOpenedPopup,
  ackPopup,
} from "./browserPopupNative";

export interface BrowserSlice {
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

function browserTabTitle(url: string, documentTitle?: string | null) {
  let host: string;
  try {
    host = new URL(url).host.slice(0, 48);
  } catch {
    return null;
  }
  const title =
    typeof documentTitle === "string"
      ? Array.from(
          Array.from(documentTitle.slice(0, 512), (character) => {
            const code = character.codePointAt(0) ?? 0;
            if (
              code === 0x061c ||
              code === 0x200e ||
              code === 0x200f ||
              (code >= 0x202a && code <= 0x202e) ||
              (code >= 0x2066 && code <= 0x2069)
            )
              return "";
            return code < 32 || (code >= 127 && code <= 159) ? " " : character;
          })
            .join("")
            .replace(/\s+/g, " ")
            .trim(),
        )
          .slice(0, 80)
          .join("")
      : "";
  return title || host;
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
      const projectId = get().project?.id;
      const generation = get().projectGeneration;
      if (!projectId || get().projectTransitioning)
        return await abandonNativePopup(intent);
      await get().flushActiveEditor();
      if (!matchesProjectIntent(get(), generation, projectId)) {
        await abandonNativePopup(intent);
        throw new Error("Project changed");
      }
      const opened = await browserOpenPopup(intent.id, intent.openerTabId);
      if (
        opened.openerTabId !== intent.openerTabId ||
        !opened.tabId ||
        !opened.url
      )
        throw new Error("Popup response invalid");
      if (!matchesProjectIntent(get(), generation, projectId)) {
        await abandonOpenedPopup(intent.id, opened);
        throw new Error("Project changed");
      }
      const inserted = !get().tabs.some((tab) => tab.id === opened.tabId);
      if (inserted) {
        const openerIndex = get().tabs.findIndex(
          (tab) => tab.id === opened.openerTabId && tab.kind === "browser",
        );
        if (openerIndex < 0) {
          await abandonOpenedPopup(intent.id, opened);
          throw new Error("Popup opener closed");
        }
        set((state) => {
          const tabs = [...state.tabs];
          tabs.splice(openerIndex + 1, 0, {
            id: opened.tabId,
            kind: "browser",
            title: browserTabTitle(opened.url) ?? "New tab",
            position: openerIndex + 1,
            pinned: false,
            dirty: false,
            openerTabId: opened.openerTabId,
          });
          return {
            tabs: tabs.map((tab, position) => ({ ...tab, position })),
            activeTabId: opened.tabId,
            activeEditor: null,
          };
        });
      }
      await ackPopup(set, get, intent.id, opened, inserted);
      finishedPopups.add(intent.id);
      if (finishedPopups.size > 32) {
        const first = finishedPopups.values().next().value;
        if (first !== undefined) finishedPopups.delete(first);
      }
    } catch {
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
                  title = "New tab";
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
