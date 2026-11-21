import { invokeNative } from "../ports/nativeInvoke";
import type {
  AcpChatStatus,
  GetAcpChatPageRequest,
  TurnState,
  AcpProjectionChangedEvent,
} from "../domain/acp";
import {
  isOneOf,
  isRecord,
  safeIdentifier,
  safeText,
  safeTimestamp,
} from "./acpDesktopGuards";

export function safeProjectionChanged(
  value: unknown,
): AcpProjectionChangedEvent | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const projectionRevision =
    typeof value.projectionRevision === "string"
      ? value.projectionRevision
      : null;
  if (
    !projectId ||
    !chatId ||
    !sessionId ||
    !projectionRevision ||
    !/^[1-9][0-9]{0,19}$/.test(projectionRevision)
  ) {
    return null;
  }
  return {
    projectId,
    chatId,
    sessionId,
    projectionRevision,
  } as AcpProjectionChangedEvent;
}

export function safeChatStatus(value: unknown): AcpChatStatus | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const title = safeText(value.title, 8192);
  if (!projectId || !chatId || title === null) return null;
  let latestTurn: AcpChatStatus["latestTurn"] = null;
  if (value.latestTurn !== null) {
    const turn = value.latestTurn;
    if (
      !isRecord(turn) ||
      !isOneOf<TurnState>(turn.state, [
        "queued",
        "sending",
        "running",
        "waitingModel",
        "waitingTool",
        "waitingApproval",
        "waitingUser",
        "cancelling",
        "completed",
        "interrupted",
        "cancelled",
        "failed",
        "deliveryUnknown",
      ])
    )
      return null;
    const updatedAt = safeTimestamp(turn.updatedAt);
    const createdAt = safeTimestamp(turn.createdAt);
    const terminalAt =
      turn.terminalAt === null ? null : safeTimestamp(turn.terminalAt);
    if (
      updatedAt === null ||
      createdAt === null ||
      (terminalAt === null && turn.terminalAt !== null)
    )
      return null;
    latestTurn = { state: turn.state, updatedAt, createdAt, terminalAt };
  }
  return {
    projectId: projectId as AcpChatStatus["projectId"],
    chatId: chatId as AcpChatStatus["chatId"],
    title,
    latestTurn,
  };
}

export async function getChatStatus(
  request: Omit<GetAcpChatPageRequest, "cursor">,
): Promise<AcpChatStatus> {
  const status = safeChatStatus(
    await invokeNative("get_acp_chat_status", { request }),
  );
  if (
    !status ||
    status.projectId !== request.projectId ||
    status.chatId !== request.chatId
  )
    throw new Error("Invalid bounded ACP chat status");
  return status;
}
