import { beforeEach, describe, expect, it, vi } from "vitest";

const getPreferences = vi.fn();
const setPreferences = vi.fn();

vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: () => getPreferences(),
  browserSetPreferences: (value: unknown) => setPreferences(value),
}));

import {
  admitBrowserPreferences,
  DEFAULT_BROWSER_PREFERENCES,
  ensureBrowserPreferences,
  MAX_AUTOMATIC_LOADS,
  resetBrowserPreferencesForTests,
  useBrowserPreferencesStore,
} from "./browserPreferencesStore";

beforeEach(() => {
  vi.clearAllMocks();
  resetBrowserPreferencesForTests();
});

describe("browser preferences", () => {
  it("shows only the host by default", () => {
    expect(DEFAULT_BROWSER_PREFERENCES).toEqual({ showFullUrl: false });
  });

  it("admits only the known boolean field", () => {
    expect(admitBrowserPreferences({ showFullUrl: true, extra: "x" })).toEqual({
      showFullUrl: true,
    });
    expect(admitBrowserPreferences({ showFullUrl: "yes" })).toEqual({
      showFullUrl: false,
    });
    expect(admitBrowserPreferences(null)).toEqual({ showFullUrl: false });
  });

  it("restores the stored preferences from Rust", async () => {
    getPreferences.mockResolvedValue({ showFullUrl: true });
    await useBrowserPreferencesStore.getState().restore();
    expect(useBrowserPreferencesStore.getState()).toMatchObject({
      preferences: { showFullUrl: true },
      ready: true,
      error: null,
    });
  });

  it("stays unready and on the default when loading fails", async () => {
    getPreferences.mockRejectedValue(new Error("no desktop"));
    await useBrowserPreferencesStore.getState().restore();
    expect(useBrowserPreferencesStore.getState()).toMatchObject({
      preferences: { showFullUrl: false },
      ready: false,
      error: "Browser settings could not be loaded.",
    });
  });

  it("ignores changes before the stored preferences load", () => {
    useBrowserPreferencesStore.getState().setShowFullUrl(true);
    expect(setPreferences).not.toHaveBeenCalled();
    expect(useBrowserPreferencesStore.getState().preferences.showFullUrl).toBe(
      false,
    );
  });

  it("persists a change and rolls back when the save fails", async () => {
    getPreferences.mockResolvedValue({ showFullUrl: false });
    await useBrowserPreferencesStore.getState().restore();
    setPreferences.mockResolvedValueOnce({ showFullUrl: true });
    useBrowserPreferencesStore.getState().setShowFullUrl(true);
    expect(setPreferences).toHaveBeenCalledWith({ showFullUrl: true });
    await useBrowserPreferencesStore.getState().flush();
    expect(useBrowserPreferencesStore.getState().preferences.showFullUrl).toBe(
      true,
    );

    setPreferences.mockRejectedValueOnce(new Error("disk"));
    useBrowserPreferencesStore.getState().setShowFullUrl(false);
    await useBrowserPreferencesStore.getState().flush();
    expect(useBrowserPreferencesStore.getState()).toMatchObject({
      preferences: { showFullUrl: true },
      error: "Browser settings could not be saved.",
    });
  });

  it("tries a failed load again on the next reader, a bounded number of times", async () => {
    getPreferences.mockRejectedValue(new Error("busy"));
    for (let attempt = 0; attempt < MAX_AUTOMATIC_LOADS + 2; attempt += 1) {
      ensureBrowserPreferences();
      await vi.waitFor(() =>
        expect(useBrowserPreferencesStore.getState().error).not.toBeNull(),
      );
    }
    expect(getPreferences).toHaveBeenCalledTimes(MAX_AUTOMATIC_LOADS);

    // An explicit Retry still loads, and a success re-arms the automatic loads.
    getPreferences.mockResolvedValue({ showFullUrl: true });
    await useBrowserPreferencesStore.getState().restore();
    expect(useBrowserPreferencesStore.getState()).toMatchObject({
      ready: true,
      error: null,
      preferences: { showFullUrl: true },
    });
    ensureBrowserPreferences();
    expect(getPreferences).toHaveBeenCalledTimes(MAX_AUTOMATIC_LOADS + 1);
  });

  it("shares one load between readers that mount together", async () => {
    getPreferences.mockResolvedValue({ showFullUrl: false });
    ensureBrowserPreferences();
    ensureBrowserPreferences();
    await useBrowserPreferencesStore.getState().restore();
    expect(getPreferences).toHaveBeenCalledTimes(1);
  });
});
