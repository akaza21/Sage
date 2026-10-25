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

export function abandonOpenedPopup(id: number, opened: BrowserPopupOpen) {
  return browserPopupAbandon(id, opened.openerTabId, opened.tabId);
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
    if (inserted) reconcileNativeClosedBrowserTabs(set, get, [opened.tabId]);
    throw error;
  }
}
