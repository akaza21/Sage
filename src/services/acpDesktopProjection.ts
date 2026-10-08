import type { AcpProjectionChangedEvent } from "../domain/acp";
import { isRecord, safeIdentifier } from "./acpDesktopResponses";

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
