import { afterEach, describe, expect, it, vi } from "vitest";

import type { WorkbenchState } from "./store";

describe("workbench close", () => {
  afterEach(() => {
    vi.doUnmock("./workspaceNotifyLifecycle");
    vi.restoreAllMocks();
    vi.resetModules();
  });

  it("closes without loading the watcher when no Project was bound", async () => {
    vi.resetModules();
    const watcherLoaded = vi.fn();
    vi.doMock("./workspaceNotifyLifecycle", () => {
      watcherLoaded();
      return {};
    });
    const { getWorkbenchPorts } = await import("../ports/workbenchRuntime");
    const project = getWorkbenchPorts().project;
    const persist = vi
      .spyOn(project, "persistProjectLayoutAndClose")
      .mockResolvedValue();
    const unwatch = vi
      .spyOn(project, "unwatchWorkspaceNotify")
      .mockResolvedValue();
    const { useWorkbenchStore } = await import("./store");
    const { closeLiveWorkbench } = await import("./workbenchClose");
    const state = useWorkbenchStore.getState();
    const get = (): WorkbenchState => ({
      ...state,
      flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    });

    await closeLiveWorkbench(get);

    expect(persist).toHaveBeenCalledTimes(1);
    expect(unwatch).not.toHaveBeenCalled();
    expect(watcherLoaded).not.toHaveBeenCalled();
  });
});
