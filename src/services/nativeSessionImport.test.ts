import { beforeEach, expect, it, vi } from "vitest";

const { invokeNative } = vi.hoisted(() => ({ invokeNative: vi.fn() }));
vi.mock("../ports/nativeInvoke", () => ({ invokeNative }));

import { listNativeSessions, readImportedHistory } from "./nativeSessionImport";

const lister = { sessionId: "session-lister", runtimeEpoch: "3" };

beforeEach(() => invokeNative.mockReset());

it("decodes a bounded session page with its lister and rejects malformed ones", async () => {
  const session = { threadId: "thread-1", title: "Fix", updatedAt: 1700 };
  invokeNative.mockResolvedValueOnce({
    sessions: [session],
    nextCursor: "c",
    lister,
  });
  await expect(listNativeSessions("project-1", "b", lister)).resolves.toEqual({
    sessions: [session],
    nextCursor: "c",
    lister,
  });
  expect(invokeNative).toHaveBeenCalledWith("list_native_sessions", {
    projectId: "project-1",
    cursor: "b",
    lister,
  });
  for (const value of [
    null,
    { sessions: Array(26).fill(session), nextCursor: null, lister },
    { sessions: [{ ...session, updatedAt: "now" }], nextCursor: null, lister },
    { sessions: [{ ...session, threadId: "" }], nextCursor: null, lister },
    { sessions: [], nextCursor: 3, lister },
    { sessions: [], nextCursor: null },
    { sessions: [], nextCursor: null, lister: { sessionId: "s" } },
  ]) {
    invokeNative.mockResolvedValueOnce(value);
    await expect(listNativeSessions("project-1", null, null)).rejects.toThrow();
  }
});

it("reads imported history as settled Turns of the imported session", async () => {
  invokeNative.mockResolvedValueOnce(null);
  await expect(readImportedHistory("project-1", "chat-1")).resolves.toBeNull();
  invokeNative.mockResolvedValueOnce({
    sessionId: "session-1",
    runtimeEpoch: "1",
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
      {
        id: "turn-2",
        status: "inProgress",
        items: [
          {
            id: "think-1",
            lifecycle: "started",
            payload: { kind: "thoughtSummary", payload: { summary: "Plan" } },
          },
        ],
      },
    ],
  });
  const history = await readImportedHistory("project-1", "chat-1");
  expect(history?.retentionLimited).toBe(true);
  expect(history?.turns[0]).toMatchObject({
    id: "turn-1",
    sessionId: "session-1",
    runtimeEpoch: "1",
    ordinal: 1,
    state: "completed",
  });
  expect(history?.turns[0]?.items[0]).toMatchObject({
    kind: "text",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    payload: { role: "assistant", text: "Done" },
  });
  expect(history?.turns[1]).toMatchObject({ state: "interrupted" });
  expect(history?.turns[1]?.items[0]).toMatchObject({ lifecycle: "cancelled" });
  invokeNative.mockResolvedValueOnce({
    sessionId: "session-1",
    runtimeEpoch: "1",
    turns: "x",
    retentionLimited: false,
  });
  await expect(readImportedHistory("project-1", "chat-1")).rejects.toThrow();
});
