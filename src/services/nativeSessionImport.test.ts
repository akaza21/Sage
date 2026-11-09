import { beforeEach, expect, it, vi } from "vitest";

const { invokeNative } = vi.hoisted(() => ({ invokeNative: vi.fn() }));
vi.mock("../ports/nativeInvoke", () => ({ invokeNative }));

import { listNativeSessions, readImportedHistory } from "./nativeSessionImport";

const owner = {
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
  runtimeEpoch: "1",
};

beforeEach(() => invokeNative.mockReset());

it("decodes a bounded session page and rejects malformed ones", async () => {
  const session = { threadId: "thread-1", title: "Fix", updatedAt: 1700 };
  invokeNative.mockResolvedValueOnce({ sessions: [session], nextCursor: "c" });
  await expect(listNativeSessions("project-1", null)).resolves.toEqual({
    sessions: [session],
    nextCursor: "c",
  });
  expect(invokeNative).toHaveBeenCalledWith("list_native_sessions", {
    projectId: "project-1",
    cursor: null,
  });
  for (const value of [
    null,
    { sessions: Array(26).fill(session), nextCursor: null },
    { sessions: [{ ...session, updatedAt: "now" }], nextCursor: null },
    { sessions: [{ ...session, threadId: "" }], nextCursor: null },
    { sessions: [], nextCursor: 3 },
  ]) {
    invokeNative.mockResolvedValueOnce(value);
    await expect(listNativeSessions("project-1", null)).rejects.toThrow();
  }
});

it("reads imported history in the shared item shapes", async () => {
  invokeNative.mockResolvedValueOnce(null);
  await expect(readImportedHistory(owner)).resolves.toBeNull();
  invokeNative.mockResolvedValueOnce({
    retentionLimited: true,
    turns: [
      {
        id: "turn-1",
        status: "completed",
        items: [
          {
            id: "item-1",
            role: "assistant",
            lifecycle: "completed",
            payload: { kind: "text", payload: { text: "Done" } },
          },
        ],
      },
    ],
  });
  const history = await readImportedHistory(owner);
  expect(history?.retentionLimited).toBe(true);
  expect(history?.turns[0]?.items[0]).toMatchObject({
    kind: "text",
    chatId: "chat-1",
    turnId: "turn-1",
    payload: { role: "assistant", text: "Done" },
  });
  invokeNative.mockResolvedValueOnce({ turns: "x", retentionLimited: false });
  await expect(readImportedHistory(owner)).rejects.toThrow();
});
