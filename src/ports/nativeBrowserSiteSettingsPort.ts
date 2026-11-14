import { invokeNative } from "./nativeInvoke";
import type { BrowserPermissionKind } from "./nativeBrowserPort";

/**
 * Saved browser site permissions (Settings > Browser). Kept apart from
 * `nativeBrowserPort` so only the lazy Settings chunk carries the list calls.
 */

export interface BrowserSiteSetting {
  origin: string;
  kind: BrowserPermissionKind;
  decision: "allow" | "deny";
}

export function browserSiteSettings(): Promise<unknown> {
  return invokeNative("browser_site_settings");
}

export interface BrowserSiteSettingsView {
  settings: BrowserSiteSetting[];
  /** Sites whose pages may still use an ended Allow until their documents are
   * replaced. */
  staleSites: string[];
}

export function browserSiteSettingSet(
  origin: string,
  kind: BrowserPermissionKind,
  decision: "allow" | "deny",
): Promise<unknown> {
  return invokeNative("browser_site_setting_set", { origin, kind, decision });
}

export function browserSiteSettingsResetAll(): Promise<unknown> {
  return invokeNative("browser_site_settings_reset_all");
}

export function browserSiteReloadTabs(): Promise<unknown> {
  return invokeNative("browser_site_reload_tabs");
}
