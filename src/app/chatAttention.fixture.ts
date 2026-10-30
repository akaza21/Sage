import { vi } from "vitest";
import type { ChatAttentionV1 } from "../domain/chatAttention";
import { chatAttention } from "../services/chatAttention";
import { chatAttentionStore } from "./chatAttentionStore";

export async function seedChatAttention(initial: ChatAttentionV1[]) {
  let rows = initial;
  const change = (
    projectId: string,
    chatId: string,
    update: (row: ChatAttentionV1) => ChatAttentionV1,
  ) => {
    const index = rows.findIndex(
      (row) => row.projectId === projectId && row.chatId === chatId,
    );
    rows = rows.map((row, offset) => (offset === index ? update(row) : row));
    return rows[index];
  };
  vi.spyOn(chatAttention, "get").mockImplementation(async (projectId, ids) =>
    rows.filter(
      (row) => row.projectId === projectId && ids.includes(row.chatId),
    ),
  );
  vi.spyOn(chatAttention, "subscribe").mockResolvedValue(() => undefined);
  const ack = vi
    .spyOn(chatAttention, "ack")
    .mockImplementation(async (project, chat, observed) =>
      change(project, chat, (row) => ({
        ...row,
        readPosition: observed,
        revision: String(BigInt(row.revision) + 1n),
      })),
    );
  vi.spyOn(chatAttention, "markUnread").mockImplementation(
    async (project, chat) =>
      change(project, chat, (row) => ({
        ...row,
        activityPosition: String(BigInt(row.activityPosition) + 1n),
        forcedUnreadPosition: String(BigInt(row.activityPosition) + 1n),
        revision: String(BigInt(row.revision) + 1n),
      })),
  );
  const projects = [...new Set(rows.map((row) => row.projectId))];
  await chatAttentionStore.connect(
    projects.map((projectId) => ({
      projectId,
      chatIds: rows
        .filter((row) => row.projectId === projectId)
        .map((row) => row.chatId),
    })),
  );
  return {
    ack,
    async update(next: ChatAttentionV1[]) {
      rows = next;
      await chatAttentionStore.refresh();
    },
  };
}
