import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import {
  resetSubagentLiveness,
  useChildReads,
  type ChildRead,
} from "./subagentLiveness";

const reads = vi.hoisted(() => [] as string[]);
const held = vi.hoisted(() => [] as (() => void)[]);
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  isTauriRuntime: () => false,
  invokeNative: async (
    _command: string,
    { request }: { request: { agentKey: string } },
  ) => {
    reads.push(request.agentKey);
    await new Promise<void>((resolve) => held.push(resolve));
    return {
      kind: "current",
      snapshot: {
        childThreadId: request.agentKey,
        parentThreadId: "parent",
        retentionLimited: false,
        turns: [{ id: "child-turn", status: "inProgress", items: [] }],
      },
    };
  },
}));

afterEach(() => {
  cleanup();
  reads.length = 0;
  held.length = 0;
  vi.restoreAllMocks();
  resetSubagentLiveness();
});

it("gives every waiting child a read while slow reads meet constant invalidations", async () => {
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const read = (agentKey: string): ChildRead => {
    const request = {
      projectId: "project",
      projectGeneration: "3",
      chatId: "chat",
      sessionId: "session",
      runtimeEpoch: "2",
      itemId: `spawn-${agentKey}`,
      agentKey,
    };
    return { identity: Object.values(request).join("\u0000"), request };
  };
  const views = ["child-0", "child-1", "child-2"].map((key) =>
    renderHook(({ epoch }) => useChildReads(read(key), epoch, true, () => {}), {
      initialProps: { epoch: 1 },
    }),
  );
  await waitFor(() => expect(reads).toEqual(["child-0"]));
  for (let epoch = 2; reads.length < 6; epoch += 1) {
    now += 2_000;
    for (const view of views) view.rerender({ epoch });
    const count = reads.length;
    held.shift()!();
    await waitFor(() => expect(reads.length).toBe(count + 1));
  }
  expect(reads).toEqual([
    "child-0",
    "child-1",
    "child-2",
    "child-0",
    "child-1",
    "child-2",
  ]);
});
