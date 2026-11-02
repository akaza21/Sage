import { invokeNative } from "./nativeInvoke";
import type { BrowserPermissionKind } from "./nativeBrowserPort";

/**
 * Saved browser site permissions (Settings > Browser). Kept apart from
 * `nativeBrowserPort` so only the lazy Settings chunk carries the list calls.
 */

/** One saved site answer; a site with no row asks again. */
export interface BrowserSiteSetting {
  origin: string;
  kind: BrowserPermissionKind;
  decision: "allow" | "deny";
}

export function browserSiteSettings(): Promise<unknown> {
  return invokeNative("browser_site_settings");
}

/**
 * Settings' view of the saved answers, and the sites of tabs whose page may
 * still use an Allow that ended because it has not reloaded yet.
 */
export interface BrowserSiteSettingsView {
  settings: BrowserSiteSetting[];
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

/** Reloads every tab still marked stale and waits for each to reload. */
export function browserSiteReloadTabs(): Promise<unknown> {
  return invokeNative("browser_site_reload_tabs");
}
