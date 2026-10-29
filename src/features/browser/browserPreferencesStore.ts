import { useEffect } from "react";
import { create } from "zustand";

import { createSettingWriter } from "../../app/settingWriteQueue";
import {
  browserPreferences,
  browserSetPreferences,
  type BrowserPreferences,
} from "../../ports/nativeBrowserPreferencesPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";

export type { BrowserPreferences };

const LOAD_FAILURE = "Browser settings could not be loaded.";
const SAVE_FAILURE = "Browser settings could not be saved.";

export const DEFAULT_BROWSER_PREFERENCES: BrowserPreferences = Object.freeze({
  showFullUrl: false,
});

/** Keep only the known fields; Rust validates the same record again. */
export function admitBrowserPreferences(input: unknown): BrowserPreferences {
  const source =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return {
    showFullUrl:
      typeof source.showFullUrl === "boolean"
        ? source.showFullUrl
        : DEFAULT_BROWSER_PREFERENCES.showFullUrl,
  };
}

interface BrowserPreferencesStore {
  preferences: BrowserPreferences;
  ready: boolean;
  error: string | null;
  restore: () => Promise<void>;
  flush: () => Promise<void>;
  setShowFullUrl: (showFullUrl: boolean) => void;
}

const writes = createSettingWriter<BrowserPreferences>(
  DEFAULT_BROWSER_PREFERENCES,
);
let restoring: Promise<void> | null = null;

export const useBrowserPreferencesStore = create<BrowserPreferencesStore>(
  (set, get) => ({
    preferences: DEFAULT_BROWSER_PREFERENCES,
    ready: false,
    error: null,
    restore: () => {
      restoring ??= (async () => {
        try {
          const preferences = admitBrowserPreferences(
            await browserPreferences(),
          );
          writes.persisted(preferences);
          set({ preferences, ready: true, error: null });
        } catch {
          set({ ready: false, error: LOAD_FAILURE });
        }
      })().finally(() => {
        restoring = null;
      });
      return restoring;
    },
    flush: () => writes.idle(),
    setShowFullUrl: (showFullUrl) => {
      if (!get().ready) return;
      const preferences = { ...get().preferences, showFullUrl };
      set({ preferences, error: null });
      writes.write(preferences, browserSetPreferences, (persisted) =>
        set({ preferences: persisted, error: SAVE_FAILURE }),
      );
    },
  }),
);

/** Load the stored choices once, for the first reader that needs them. */
export function ensureBrowserPreferences(): void {
  if (!isTauriRuntime()) return;
  const { ready, error } = useBrowserPreferencesStore.getState();
  if (!ready && !error) void useBrowserPreferencesStore.getState().restore();
}

/** The address bar's Show full URL choice, loaded on first use. */
export function useShowFullUrl(): boolean {
  useEffect(ensureBrowserPreferences, []);
  return useBrowserPreferencesStore((state) => state.preferences.showFullUrl);
}
