import { invokeNative } from "./nativeInvoke";

/** The Browser settings record Rust stores; see `settings_store/browser_preferences.rs`. */
export interface BrowserPreferences {
  showFullUrl: boolean;
}

export function browserPreferences(): Promise<unknown> {
  return invokeNative("browser_preferences");
}

/** Settings › Browser › Developer mode; see `settings_commands/browser_developer.rs`. */
export interface BrowserDeveloperMode {
  /** Only the Windows browser (WebView2) has CDP. */
  available: boolean;
  enabled: boolean;
}

export function browserDeveloperMode(): Promise<BrowserDeveloperMode> {
  return invokeNative("browser_developer_mode");
}

export function browserSetDeveloperMode(
  enabled: boolean,
): Promise<BrowserDeveloperMode> {
  return invokeNative("browser_set_developer_mode", { enabled });
}

export function browserSetPreferences(
  preferences: BrowserPreferences,
): Promise<unknown> {
  return invokeNative("browser_set_preferences", { preferences });
}
