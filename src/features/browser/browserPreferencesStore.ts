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
/** Automatic loads after a failure stop here; Retry on the page still works. */
export const MAX_AUTOMATIC_LOADS = 3;
let failedLoads = 0;

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
          failedLoads = 0;
          set({ preferences, ready: true, error: null });
        } catch {
          failedLoads += 1;
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

export function ensureBrowserPreferences(): void {
  if (!isTauriRuntime()) return;
  if (useBrowserPreferencesStore.getState().ready) return;
  if (failedLoads >= MAX_AUTOMATIC_LOADS) return;
  void useBrowserPreferencesStore.getState().restore();
}

export function resetBrowserPreferencesForTests(): void {
  failedLoads = 0;
  useBrowserPreferencesStore.setState({
    preferences: DEFAULT_BROWSER_PREFERENCES,
    ready: false,
    error: null,
  });
}

export function useShowFullUrl(): boolean {
  useEffect(ensureBrowserPreferences, []);
  return useBrowserPreferencesStore((state) => state.preferences.showFullUrl);
}
