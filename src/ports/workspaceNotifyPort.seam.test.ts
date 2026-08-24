import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeNative = vi.fn();

vi.mock("./nativeInvoke", () => ({
  invokeNative: (command: string, args?: Record<string, unknown>) =>
    invokeNative(command, args),
  isTauriRuntime: () => false,
}));

const bind = {
  stamp: {
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 7,
  },
  roots: [{ rootId: "root-a" }],
};

describe("workspace notify owner-port wire", () => {
  beforeEach(() => {
    invokeNative.mockReset();
    invokeNative.mockResolvedValue({ generation: 1 });
  });

  it("native watch forwards the unchanged bind envelope", async () => {
    const { createNativeWorkbenchPorts } =
      await import("./nativeWorkbenchPorts");
    await createNativeWorkbenchPorts().project.watchWorkspaceNotify(bind);
    expect(invokeNative).toHaveBeenCalledWith("watch_workspace_notify", {
      value: bind,
    });
  });

  it("native unwatch forwards the unchanged projectId envelope", async () => {
    const { createNativeWorkbenchPorts } =
      await import("./nativeWorkbenchPorts");
    invokeNative.mockResolvedValue(undefined);
    await createNativeWorkbenchPorts().project.unwatchWorkspaceNotify({
      projectId: "project-a",
    });
    expect(invokeNative).toHaveBeenCalledWith("unwatch_workspace_notify", {
      value: { projectId: "project-a" },
    });
  });

  it("browser watch and unwatch reject", async () => {
    const { createBrowserWorkbenchPorts } =
      await import("./browserWorkbenchPorts");
    const project = createBrowserWorkbenchPorts().project;
    await expect(project.watchWorkspaceNotify(bind)).rejects.toThrow(
      "workspace notify is not available in the browser adapter",
    );
    await expect(
      project.unwatchWorkspaceNotify({ projectId: "project-a" }),
    ).rejects.toThrow(
      "workspace notify is not available in the browser adapter",
    );
  });
});
