import type { StateCreator } from "zustand";

import { errorMessage } from "../domain/errors";
import {
  closeBrowser,
  openBrowser,
  type BrowserNavigation,
} from "../ports/nativeBrowserPort";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { browserFaviconDataUrl } from "../features/browser/browserAddress";
import {
  clearBrowserAgentPresence,
  recordBrowserAgentOperation,
} from "./browserAgentActivityStore";
import type { BrowserOperationProjection } from "../domain/browserOperation";
import { matchesProjectIntent } from "./projectIntent";
import {
  consumeBrowserAlreadyClosed,
  isBrowserCloseInFlight,
  reconcileNativeClosedBrowserTabs,
} from "./tabClosing";
import type { WorkbenchState } from "./store";

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

function browserTabTitle(
  url: string,
  documentTitle?: string | null,
): string | null {
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
  if (isTauriRuntime()) {
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
            await listen<string>("browser:popup-request", (event) => {
              void get().openBrowser(event.payload);
            }),
          );
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
                  .catch((error) => set({ error: errorMessage(error) }));
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
          .catch((error) => {
            set((current) => ({
              tabs: current.tabs.some((item) => item.id === tab.id)
                ? current.tabs
                : [...current.tabs, tab],
              error: `Couldn't close browser — try again: ${errorMessage(error)}`,
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
      } catch (error) {
        set({ error: errorMessage(error) });
      }
    },
  };
};
