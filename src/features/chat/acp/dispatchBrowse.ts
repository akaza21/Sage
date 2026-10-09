import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import {
  extractAdmittedBrowseUrl,
  harnessBrowseOwner,
} from "../../preview/harnessBrowse";
import { CURATED_BROWSE_TOOL_NAME, invokeBrowseTool } from "./toolHost";

function toolBrowseUrl(item: AcpItem): string | null {
  if (item.kind !== "toolCall") {
    return null;
  }
  const payload = item.payload;
  const presentation = payload.presentation;
  const label = presentation.label;
  if (label !== CURATED_BROWSE_TOOL_NAME) return null;
  const summary =
    ("summary" in presentation ? presentation.summary : null) ??
    ("inputSummary" in payload ? payload.inputSummary : null) ??
    ("outputSummary" in payload ? payload.outputSummary : null);
  return extractAdmittedBrowseUrl(summary ?? "")?.href ?? null;
}

export function dispatchBrowseFromAcpPage(
  page: Pick<
    AcpChatPage,
    "projectId" | "chatId" | "currentSessionId" | "sessions" | "turns"
  >,
): void {
  if (!page.currentSessionId) return;
  const owner = harnessBrowseOwner();
  const currentSession = page.sessions.find(
    (session) => session.id === page.currentSessionId,
  );
  if (
    !owner ||
    owner.projectId !== page.projectId ||
    owner.chatId !== page.chatId ||
    owner.sessionId !== page.currentSessionId ||
    currentSession?.runtimeEpoch !== owner.runtimeEpoch
  ) {
    return;
  }
  for (const turn of page.turns) {
    for (const item of turn.items) {
      if (
        item.projectId !== page.projectId ||
        item.chatId !== page.chatId ||
        item.sessionId !== page.currentSessionId ||
        turn.sessionId !== page.currentSessionId ||
        turn.runtimeEpoch !== owner.runtimeEpoch ||
        item.runtimeEpoch !== owner.runtimeEpoch
      ) {
        continue;
      }
      const url = toolBrowseUrl(item);
      if (!url) continue;
      invokeBrowseTool(CURATED_BROWSE_TOOL_NAME, {
        owner,
        url,
      });
    }
  }
}
