import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";

export function textItem(): Extract<AcpItem, { kind: "text" }> {
  return {
    id: "item-text",
    kind: "text",
    origin: "primary",
    createdAt: Date.UTC(2026, 7, 31, 9, 5),
    payload: { role: "assistant", text: "Normalized answer" },
  } as unknown as Extract<AcpItem, { kind: "text" }>;
}

export function page(
  state: TurnProjection["state"],
  terminalReason?: TurnProjection["terminalReason"],
): AcpChatPage {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Durable chat",
    projectionRevision: "12",
    currentSessionId: "session-1",
    sessions: [{ id: "session-1", runtimeEpoch: "2" }] as never,
    nextCursor: null,
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "2",
        ordinal: 1,
        state,
        revision: "3",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: state === "completed" ? 2 : null,
        ...(terminalReason ? { terminalReason } : {}),
        items: [textItem()],
        activities: [],
      } as unknown as TurnProjection,
    ],
  } as unknown as AcpChatPage;
}
