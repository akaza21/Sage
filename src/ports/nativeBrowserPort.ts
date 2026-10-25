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
  "back" | "forward" | "reload" | "stop" | "annotateOn" | "annotateOff";

export function browserControl(
  tabId: string,
  action: BrowserControlAction,
): Promise<void> {
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

export interface BrowserDownload {
  tabId: string;
  id: number;
  source: string;
  fileName: string;
  state:
    | "choosingDestination"
    | "downloading"
    | "completed"
    | "interrupted"
    | "canceled"
    | "error";
  receivedBytes: number;
  totalBytes: number | null;
}

export function browserDownloads(tabId: string): Promise<BrowserDownload[]> {
  return invokeNative("browser_downloads", { tabId });
}

export function browserCancelDownload(
  tabId: string,
  id: number,
): Promise<void> {
  return invokeNative("browser_cancel_download", { tabId, id });
}

export function browserShowDownloadInFolder(
  tabId: string,
  id: number,
): Promise<void> {
  return invokeNative("browser_show_download_in_folder", { tabId, id });
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

export function browserPermissionRevoke(
  tabId: string,
  origin: string,
  kind: BrowserPermissionKind,
): Promise<void> {
  return invokeNative("browser_permission_revoke", { tabId, origin, kind });
}
