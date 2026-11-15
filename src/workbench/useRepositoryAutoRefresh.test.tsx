import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useWorkbenchStore, type WorkbenchState } from "../app/store";
import { DEFAULT_EDITOR_SETTINGS } from "../domain/editorSettingsCatalogue";
import type { ProjectRepositoryStatus } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useRepositoryAutoRefresh } from "./useRepositoryAutoRefresh";

vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  isTauriRuntime: () => true,
}));

const inventory = (
  branch: string,
  edited: string[] = [],
): ProjectRepositoryStatus => ({
  roots: [
    {
      rootId: "root-a",
      state: "repository",
      branch,
      changes: edited.map((relativePath) => ({
        resource: { rootId: "root-a", relativePath },
        indexStatus: "unchanged",
        worktreeStatus: "modified",
        additions: 1,
        deletions: 0,
      })),
      truncated: false,
      elapsedMs: 1,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 1,
});

let onDisk = inventory("main");
let headStamp = "main";
const checkOut = (branch: string) => {
  onDisk = inventory(branch);
  headStamp = branch;
};
const editFile = (relativePath: string) => {
  const root = onDisk.roots[0]!;
  onDisk = inventory(root.branch!, [
    ...root.changes.map((change) => change.resource.relativePath),
    relativePath,
  ]);
};

const branchInStore = () =>
  useWorkbenchStore.getState().repositories.roots[0]?.branch ?? null;
const editedInStore = () =>
  useWorkbenchStore
    .getState()
    .repositories.roots[0]?.changes.map(
      (change) => change.resource.relativePath,
    ) ?? [];

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
  onDisk = inventory("main");
  headStamp = "main";
  vi.spyOn(desktop, "getRepositoryChangeStamp").mockImplementation(
    async () => headStamp,
  );
  vi.spyOn(desktop, "getRepositoryStatus").mockImplementation(async () =>
    structuredClone(onDisk),
  );
  useWorkbenchStore.setState({
    project: { id: "project-a" } as WorkbenchState["project"],
    repositories: inventory("main"),
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState({ project: null, projectTransitioning: false });
  setAutoRefresh(true);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("useRepositoryAutoRefresh", () => {
  it("picks up an outside checkout while the setting is on and stops when it is turned off", async () => {
    setAutoRefresh(true);
    const { rerender } = renderHook(() => useRepositoryAutoRefresh());
    await vi.advanceTimersByTimeAsync(500);

    checkOut("feature");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(branchInStore()).toBe("feature");

    setAutoRefresh(false);
    rerender();
    checkOut("release");
    editFile("src/app.ts");
    window.dispatchEvent(new Event("focus"));
    // Longer than the slowest poll could wait.
    await vi.advanceTimersByTimeAsync(130_000);
    expect(branchInStore()).toBe("feature");
    expect(editedInStore()).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("applies the refresh it had to put off once a busy store is free", async () => {
    setAutoRefresh(true);
    useWorkbenchStore.setState({ projectTransitioning: true });
    renderHook(() => useRepositoryAutoRefresh());

    editFile("src/app.ts");
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(400);
    expect(editedInStore()).toEqual([]);

    useWorkbenchStore.setState({ projectTransitioning: false });
    // The retry (500 ms after the skipped refresh) and its debounce (300 ms)
    // land before the first poll, which waits 2 s from mount.
    await vi.advanceTimersByTimeAsync(800);
    expect(editedInStore()).toEqual(["src/app.ts"]);
  });

  it("leaves the store's Git state alone with the setting off", async () => {
    setAutoRefresh(false);
    renderHook(() => useRepositoryAutoRefresh());

    checkOut("feature");
    editFile("src/app.ts");
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(130_000);
    expect(branchInStore()).toBe("main");
    expect(editedInStore()).toEqual([]);
  });
});
