import type { StoreApi } from "zustand";

import {
  browserPopupAbandon,
  browserPopupAck,
  browserOpenPopup,
  type BrowserPopupIntent,
  type BrowserPopupOpen,
} from "../ports/nativeBrowserPort";
import { browserTabTitle } from "../domain/browserTabTitle";
import { matchesProjectIntent } from "./projectIntent";
import { positionTab } from "./tabState";
import type { WorkbenchState } from "./store";
import { reconcileNativeClosedBrowserTabs } from "./tabClosing";

/** A Windows native child is already a live hidden page holding a tab slot;
 * release it whenever the chrome will not adopt it. */
export async function abandonNativePopup(intent: BrowserPopupIntent) {
  const child = intent.nativeTabId;
  if (typeof child === "string")
    await browserPopupAbandon(intent.id, intent.openerTabId, child);
}

async function abandonOpenedPopup(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  id: number,
  opened: BrowserPopupOpen,
  inserted: boolean,
) {
  await browserPopupAbandon(id, opened.openerTabId, opened.tabId);
  if (inserted) reconcileNativeClosedBrowserTabs(set, get, [opened.tabId]);
}

export function popupAdoption(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  intent: BrowserPopupIntent,
) {
  return {
    opened: null as BrowserPopupOpen | null,
    inserted: false,
    cleanupAttempted: false,
    async abandon() {
      this.cleanupAttempted = true;
      const opened = this.opened;
      if (!opened) return abandonNativePopup(intent);
      await abandonOpenedPopup(set, get, intent.id, opened, this.inserted);
    },
  };
}

/** Acknowledges an adopted popup. If the ack fails for a tab this call just
 * inserted (for example the native deadline closed it first), the tab is
 * removed again so the chrome never shows a tab without a webview. */
export async function ackPopup(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  id: number,
  opened: BrowserPopupOpen,
  inserted: boolean,
) {
  try {
    await browserPopupAck(id, opened.openerTabId, opened.tabId);
  } catch (error) {
    await abandonOpenedPopup(set, get, id, opened, inserted);
    throw error;
  }
}

/** Popup adoption is demand-loaded; startup only installs the event listeners. */
export async function adoptPopup(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  intent: BrowserPopupIntent,
): Promise<boolean> {
  const popup = popupAdoption(set, get, intent);
  try {
    const projectId = get().project?.id;
    const generation = get().projectGeneration;
    if (!projectId || get().projectTransitioning) {
      await popup.abandon();
      return false;
    }
    await get().flushActiveEditor();
    if (!matchesProjectIntent(get(), generation, projectId)) {
      throw new Error("Project changed");
    }
    const opened = await browserOpenPopup(intent.id, intent.openerTabId);
    popup.opened = opened;
    if (
      opened.openerTabId !== intent.openerTabId ||
      !opened.tabId ||
      !opened.url
    )
      throw new Error("Popup response invalid");
    if (!matchesProjectIntent(get(), generation, projectId)) {
      throw new Error("Project changed");
    }
    popup.inserted = !get().tabs.some((tab) => tab.id === opened.tabId);
    if (popup.inserted) {
      const openerIndex = get().tabs.findIndex(
        (tab) => tab.id === opened.openerTabId && tab.kind === "browser",
      );
      if (openerIndex < 0) {
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
          tabs: tabs.map(positionTab),
          activeTabId: opened.tabId,
          activeEditor: null,
        };
      });
    }
    popup.cleanupAttempted = true;
    await ackPopup(set, get, intent.id, opened, popup.inserted);
    return true;
  } catch {
    if (!popup.cleanupAttempted) {
      try {
        await popup.abandon();
      } catch {
        set({ error: "Could not close the browser popup. Try again." });
        return false;
      }
    }
    set({ error: "Could not open the browser popup. Try again." });

    return false;
  }
}
