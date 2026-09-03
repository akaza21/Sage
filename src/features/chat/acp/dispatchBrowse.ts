import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import { extractAdmittedBrowseUrl } from "../../preview/harnessBrowse";
import { CURATED_BROWSE_TOOL_NAME, createToolHost } from "./toolHost";

function toolBrowseUrl(item: AcpItem): string | null {
  if (item.kind !== "toolCall" && item.kind !== "toolUpdateResult") {
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
    "projectId" | "chatId" | "currentSessionId" | "turns"
  >,
): void {
  const host = createToolHost();
  if (
    !page.currentSessionId ||
    !host.canInvoke(CURATED_BROWSE_TOOL_NAME) ||
    !host.invoke
  ) {
    return;
  }
  for (const turn of page.turns) {
    for (const item of turn.items) {
      if (
        item.projectId !== page.projectId ||
        item.chatId !== page.chatId ||
        item.sessionId !== page.currentSessionId
      ) {
        continue;
      }
      const url = toolBrowseUrl(item);
      if (!url) continue;
      host.invoke(CURATED_BROWSE_TOOL_NAME, {
        owner: {
          projectId: page.projectId,
          chatId: page.chatId,
          sessionId: page.currentSessionId,
        },
        url,
      });
    }
  }
}
