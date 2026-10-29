import { beforeEach, describe, expect, it, vi } from "vitest";

const getPreferences = vi.fn();
const setPreferences = vi.fn();

vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: () => getPreferences(),
  browserSetPreferences: (value: unknown) => setPreferences(value),
}));

import {
  admitBrowserPreferences,
  DEFAULT_BROWSER_PREFERENCES,
  useBrowserPreferencesStore,
} from "./browserPreferencesStore";

beforeEach(() => {
  vi.clearAllMocks();
  useBrowserPreferencesStore.setState({
    preferences: DEFAULT_BROWSER_PREFERENCES,
    ready: false,
    error: null,
  });
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
});
