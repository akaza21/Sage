import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useWorkbenchStore, type WorkbenchState } from "../app/store";
import { DEFAULT_EDITOR_SETTINGS } from "../domain/editorSettingsCatalogue";
import { desktop } from "../services/desktop";
import { useRepositoryAutoRefresh } from "./useRepositoryAutoRefresh";

vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../ports/nativeInvoke")>()),
  isTauriRuntime: () => true,
}));

let stamp = vi.spyOn(desktop, "getRepositoryChangeStamp");

const setAutoRefresh = (on: boolean) =>
  useEditorSettingsStore.setState({
    settings: { ...DEFAULT_EDITOR_SETTINGS, "git.autoRefresh": on },
  });

beforeEach(() => {
  vi.useFakeTimers();
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => "visible",
  });
  stamp = vi.spyOn(desktop, "getRepositoryChangeStamp").mockResolvedValue("a");
  useWorkbenchStore.setState({
    project: { id: "project-a" } as WorkbenchState["project"],
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState({ project: null });
  setAutoRefresh(true);
  vi.useRealTimers();
});

describe("useRepositoryAutoRefresh", () => {
  it("checks Git while the setting is on and stops when it is turned off", async () => {
    setAutoRefresh(true);
    const { rerender } = renderHook(() => useRepositoryAutoRefresh());
    await vi.advanceTimersByTimeAsync(3_000);
    expect(stamp.mock.calls.length).toBeGreaterThanOrEqual(3);
    setAutoRefresh(false);
    rerender();
    const calls = stamp.mock.calls.length;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(stamp).toHaveBeenCalledTimes(calls);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does nothing with the setting off", async () => {
    setAutoRefresh(false);
    renderHook(() => useRepositoryAutoRefresh());
    await vi.advanceTimersByTimeAsync(5_000);
    expect(stamp).not.toHaveBeenCalled();
  });
});
