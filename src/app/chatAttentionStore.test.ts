import { expect, it, vi } from "vitest";
import { createChatAttentionStore } from "./chatAttentionStore";
import { attention } from "../domain/chatAttention.fixture";
import type {
  ChatAttentionEvent,
  ChatAttentionV1,
} from "../domain/chatAttention";
import { groupAgentRailProjects } from "../workbench/agentChatRail";
import { groupAgentRailByStatus } from "../workbench/agentRailStatusView";

function fixture() {
  let event!: (event: ChatAttentionEvent) => Promise<void>;
  const port = {
    get: vi.fn(async (_project: string, _chats: readonly string[]) => [
      attention(),
    ]),
    ack: vi.fn(async (_project: string, chat: string, observed: string) =>
      attention({ chatId: chat, readPosition: observed, revision: "4" }),
    ),
    markUnread: vi.fn(async () => attention()),
    subscribe: vi.fn(async (listener: typeof event) => {
      event = listener;
      return () => undefined;
    }),
  };
  const store = createChatAttentionStore(port);
  return { store, port, event: (value: ChatAttentionEvent) => event(value) };
}

it("attention rejects stale bootstrap and late project replies", async () => {
  const { store, port, event } = fixture();
  let finish!: (rows: ChatAttentionV1[]) => void;
  port.get.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const bootstrap = store.connect([
    { projectId: "project-1", chatIds: ["chat-1"] },
  ]);
  await Promise.resolve();
  port.get.mockResolvedValueOnce([
    attention({ revision: "3", activityPosition: "3" }),
  ]);
  const notification = event({
    projectId: "project-1",
    chatId: "chat-1",
    revision: "3",
  });
  finish([attention()]);
  await Promise.all([bootstrap, notification]);
  expect(store.getSnapshot().get("chat-1")?.revision).toBe("3");
  port.get.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const pending = store.refresh();
  await store.connect([{ projectId: "project-2", chatIds: ["chat-2"] }]);
  finish([attention({ revision: "9" })]);
  await pending;
  expect(store.getSnapshot().has("chat-1")).toBe(false);
});

it("attention acknowledges only presented foreground detail", async () => {
  const { store, port } = fixture();
  await store.connect([{ projectId: "project-1", chatIds: ["chat-1"] }]);
  const detail = {
    projectId: "project-1",
    chatId: "chat-1",
    observedPosition: "1",
    presented: true,
    foreground: true,
    agent: true,
    settings: false,
  };
  for (const excluded of [
    { presented: false },
    { foreground: false },
    { agent: false },
    { settings: true },
  ]) {
    await store.present({ ...detail, ...excluded });
  }
  expect(port.ack).not.toHaveBeenCalled();
  await store.present(detail);
  expect(port.ack).toHaveBeenCalledWith("project-1", "chat-1", "1");
  expect(store.getSnapshot().get("chat-1")?.readPosition).toBe("1");
});

it("attention preserves AP-C filtering ordering and bulk read", async () => {
  const { store, port } = fixture();
  port.get.mockResolvedValue([
    attention({ readPosition: "0" }),
    attention({
      chatId: "chat-2",
      terminalState: "failed",
      revision: "2",
      activityPosition: "2",
    }),
  ]);
  await store.connect([
    { projectId: "project-1", chatIds: ["chat-1", "chat-2"] },
  ]);
  const groups = groupAgentRailProjects({
    projects: [],
    currentProjectId: "project-1",
    currentChats: [
      {
        id: "chat-1",
        title: "Zulu",
        pinned: true,
        position: 0,
        status: "complete",
      },
      {
        id: "chat-2",
        title: "Alpha",
        pinned: false,
        position: 1,
        status: "failed",
      },
    ],
  });
  const sections = groupAgentRailByStatus({
    groups,
    unreadByChatId: { "chat-1": true },
    ordering: "title",
    statusFilter: "attention",
  });
  expect(sections[0].groups[0].chats.map((chat) => chat.id)).toEqual([
    "chat-2",
    "chat-1",
  ]);
  await store.markAllRead();
  expect(port.ack.mock.calls.map((call) => call.slice(1))).toEqual([
    ["chat-1", "1"],
    ["chat-2", "2"],
  ]);
});
