import type { StoreApi } from "zustand";

import {
  browserPopupAbandon,
  browserPopupAck,
  type BrowserPopupIntent,
  type BrowserPopupOpen,
} from "../ports/nativeBrowserPort";
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
