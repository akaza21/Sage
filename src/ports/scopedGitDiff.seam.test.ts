import { beforeEach, expect, it, vi } from "vitest";

const invokeNative = vi.fn();
vi.mock("./nativeInvoke", () => ({
  invokeNative: (command: string, args?: Record<string, unknown>) =>
    invokeNative(command, args),
  isTauriRuntime: () => false,
}));

beforeEach(() => {
  invokeNative.mockReset();
  invokeNative.mockResolvedValue({
    hunks: [],
    binary: false,
    truncated: false,
  });
});

it.each(["staged", "unstaged"] as const)(
  "forwards the %s scope to native Git",
  async (scope) => {
    const { createNativeWorkbenchPorts } =
      await import("./nativeWorkbenchPorts");
    const resource = { rootId: "root-a", relativePath: "tracked.txt" };
    await createNativeWorkbenchPorts().repository.getFileDiff(resource, scope);
    expect(invokeNative).toHaveBeenCalledWith("get_file_diff", {
      resource,
      scope,
    });
  },
);

it("preserves the combined diff wire contract for existing callers", async () => {
  const { createNativeWorkbenchPorts } = await import("./nativeWorkbenchPorts");
  const resource = { rootId: "root-a", relativePath: "tracked.txt" };
  await createNativeWorkbenchPorts().repository.getFileDiff(resource);
  expect(invokeNative).toHaveBeenCalledWith("get_file_diff", { resource });
});

it("does not pretend browser fixture diffs have an index/worktree split", async () => {
  const { createBrowserWorkbenchPorts } =
    await import("./browserWorkbenchPorts");
  await expect(
    createBrowserWorkbenchPorts().repository.getFileDiff(
      { rootId: "root-a", relativePath: "tracked.txt" },
      "staged",
    ),
  ).rejects.toThrow("native Git backend");
});
