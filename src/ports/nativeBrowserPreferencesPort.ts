import { invokeNative } from "./nativeInvoke";

/** The Browser settings record Rust stores; see `settings_store/browser_preferences.rs`. */
export interface BrowserPreferences {
  showFullUrl: boolean;
}

export function browserPreferences(): Promise<unknown> {
  return invokeNative("browser_preferences");
}

export function browserSetPreferences(
  preferences: BrowserPreferences,
): Promise<unknown> {
  return invokeNative("browser_set_preferences", { preferences });
}
