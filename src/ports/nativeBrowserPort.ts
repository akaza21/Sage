import { invokeNative } from "./nativeInvoke";
import type { BrowserPageContext } from "../domain/attachedContext";

export interface BrowserAttachmentStatus {
  tabId: string;
  url: string;
}

export function attachBrowserTab(
  projectId: string,
  chatId: string,
  tabId: string,
): Promise<BrowserAttachmentStatus> {
  return invokeNative("attach_browser_tab", { projectId, chatId, tabId });
}

export function detachBrowserTab(
  projectId: string,
  chatId: string,
): Promise<void> {
  return invokeNative("detach_browser_tab", { projectId, chatId });
}

export function stopBrowserTab(tabId: string): Promise<void> {
  return invokeNative("stop_browser_tab", { tabId });
}

export function browserAttachmentStatus(
  projectId: string,
  chatId: string,
): Promise<BrowserAttachmentStatus | null> {
  return invokeNative("browser_attachment_status", { projectId, chatId });
}

export function openBrowser(url?: string): Promise<string> {
  return invokeNative("browser_open", { url });
}

export interface BrowserPopupIntent {
  id: number;
  openerTabId: string;
  url: string;
  /** Present only for a Windows native child that keeps `window.opener`. */
  nativeTabId?: string;
}

export interface BrowserPopupOpen {
  tabId: string;
  openerTabId: string;
  url: string;
}

export function browserPopupPending(): Promise<BrowserPopupIntent[]> {
  return invokeNative("browser_popup_pending");
}

export function browserOpenPopup(
  intentId: number,
  openerTabId: string,
): Promise<BrowserPopupOpen> {
  return invokeNative("browser_open_popup", { intentId, openerTabId });
}

export function browserPopupAck(
  intentId: number,
  openerTabId: string,
  tabId: string,
): Promise<void> {
  return invokeNative("browser_popup_ack", { intentId, openerTabId, tabId });
}

export function browserPopupAbandon(
  intentId: number,
  openerTabId: string,
  tabId: string,
): Promise<void> {
  return invokeNative("browser_popup_abandon", {
    intentId,
    openerTabId,
    tabId,
  });
}

export function browserSuggestedSites(): Promise<string[]> {
  return invokeNative("browser_suggested_sites");
}

export function browserHistory(tabId: string): Promise<string[]> {
  return invokeNative("browser_history", { tabId });
}

export function closeBrowser(tabId: string): Promise<void> {
  return invokeNative("browser_close", { tabId });
}

export interface BrowserClearDataResult {
  closedTabIds: string[];
  cleared: boolean;
  pending: boolean;
  error: string | null;
}

export interface BrowserClearDataCompletion {
  cleared: boolean;
  error: string | null;
}

export function browserClearData(): Promise<BrowserClearDataResult> {
  return invokeNative("browser_clear_data");
}

export function positionBrowser(
  tabId: string,
  bounds: { x: number; y: number; width: number; height: number },
): Promise<void> {
  return invokeNative("browser_set_bounds", { tabId, ...bounds });
}

export function hideBrowser(tabId: string): Promise<void> {
  return invokeNative("browser_hide", { tabId });
}

/** Hides the page while a toolbar popover covers it; Annotate stays on. */
export function coverBrowser(tabId: string): Promise<void> {
  return invokeNative("browser_hide", { tabId, cover: true });
}

export interface BrowserNavigation {
  tabId: string;
  url: string;
  title?: string | null;
  favicon?: string | null;
  revision: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomPercent: number;
  error?: string | null;
  printUnavailableReason?: string | null;
  annotateUnavailableReason?: string | null;
}

export function browserStatus(tabId: string): Promise<BrowserNavigation> {
  return invokeNative("browser_status", { tabId });
}

export function browserLiveTabs(): Promise<string[]> {
  return invokeNative("browser_live_tabs");
}

export function browserPageContext(
  tabId: string,
  expectedRevision: string,
): Promise<BrowserPageContext> {
  return invokeNative("browser_page_context", { tabId, expectedRevision });
}

export function browserNavigate(tabId: string, url: string): Promise<void> {
  return invokeNative("browser_navigate", { tabId, url });
}

export type BrowserControlAction =
  | "back"
  | "forward"
  | "reload"
  | "stop"
  | "annotateOn"
  | "annotateOff"
  | "annotateCancel";

/** Where the picked element sits in the viewport it was picked in (page CSS px). */
export interface BrowserAnnotateSelection {
  /** The accessible name, or a plain word for the role. */
  label: string;
  box: { x: number; y: number; width: number; height: number };
  viewport: {
    x: number;
    y: number;
    width: number;
    height: number;
    clientWidth: number;
    clientHeight: number;
    scale: number;
  };
}

export interface BrowserAnnotateChange {
  tabId: string;
  active: boolean;
  selection?: BrowserAnnotateSelection;
  error?: string;
}

export function browserControl(
  tabId: string,
  action: BrowserControlAction,
): Promise<void> {
  return invokeNative("browser_control", { tabId, action });
}

/** The tab's live Agent attachment as Rust holds it; null when none. */
export interface BrowserAgentControl {
  projectId: string;
  chatId: string;
  /** The user took the tab over; the Agent waits until Hand back. */
  userControl: boolean;
}

/** Reads, takes over or hands back the tab; Rust answers with the state after it. */
export function browserAgentControl(
  tabId: string,
  action: "agentControl" | "agentTakeOver" | "agentHandBack",
): Promise<BrowserAgentControl | null> {
  return invokeNative("browser_control", { tabId, action });
}

export function browserZoom(tabId: string, percent: number): Promise<void> {
  return invokeNative("browser_zoom", { tabId, percent });
}

export function browserFind(
  tabId: string,
  action: "start" | "next" | "previous" | "stop",
  query?: string,
): Promise<void> {
  return invokeNative("browser_find", { tabId, action, query });
}

export function browserOpenExternal(tabId: string): Promise<void> {
  return invokeNative("browser_open_external", { tabId });
}

export function browserPrint(tabId: string): Promise<void> {
  return invokeNative("browser_print", { tabId });
}

export interface BrowserUserScreenshot {
  width: number;
  height: number;
  pngBase64: string;
}

export function browserCaptureScreenshot(
  tabId: string,
  expectedRevision: string,
): Promise<BrowserUserScreenshot> {
  return invokeNative("browser_capture_screenshot", {
    tabId,
    expectedRevision,
  });
}

export type BrowserPermissionKind =
  "camera" | "microphone" | "geolocation" | "notifications" | "clipboardRead";

export interface BrowserPermissionPrompt {
  id: number;
  tabId: string;
  origin: string;
  kind: BrowserPermissionKind;
}

export interface BrowserPermissionSnapshot {
  tabId: string;
  origin: string;
  pending: BrowserPermissionPrompt[];
  decisions: Array<{
    origin: string;
    kind: BrowserPermissionKind;
    decision: "allow" | "deny";
  }>;
  /** The page held an Allow that ended and has not reloaded since. */
  staleAccess: boolean;
}

export function browserPermissionStatus(
  tabId: string,
): Promise<BrowserPermissionSnapshot> {
  return invokeNative("browser_permission_status", { tabId });
}

export function browserPermissionResolve(
  tabId: string,
  id: number,
  allow: boolean,
): Promise<void> {
  return invokeNative("browser_permission_resolve", { tabId, id, allow });
}

/** Forgets one site's saved answer so it asks again (also Settings > Browser). */
export function browserSiteSettingReset(
  origin: string,
  kind: BrowserPermissionKind,
): Promise<unknown> {
  return invokeNative("browser_permission_reset", { origin, kind });
}
