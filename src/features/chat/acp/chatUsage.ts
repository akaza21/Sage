import type { AcpChatPage, AcpItem } from "../../../domain/acp";

export type ChatUsage = Extract<AcpItem, { kind: "usage" }>["payload"];

export function currentChatUsage(page: AcpChatPage | null): ChatUsage | null {
  if (!page?.currentSessionId) return null;
  let current: Extract<AcpItem, { kind: "usage" }> | null = null;
  for (const turn of page.turns) {
    if (turn.sessionId !== page.currentSessionId) continue;
    for (const item of turn.items) {
      if (item.kind === "usage") current = item;
    }
  }
  if (!current || current.payload.totalTokens === null) return null;
  return current.payload;
}
