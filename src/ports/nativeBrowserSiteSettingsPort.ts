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

export function browserSiteSettingSet(
  origin: string,
  kind: BrowserPermissionKind,
  decision: "allow" | "deny",
): Promise<unknown> {
  return invokeNative("browser_site_setting_set", { origin, kind, decision });
}

export function browserSiteSettingReset(
  origin: string,
  kind: BrowserPermissionKind,
): Promise<unknown> {
  return invokeNative("browser_site_setting_reset", { origin, kind });
}

export function browserSiteSettingsResetAll(): Promise<unknown> {
  return invokeNative("browser_site_settings_reset_all");
}
