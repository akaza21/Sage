import { createContext } from "react";

import type { AcpChatPage } from "../../domain/acp";

export interface ForkChatRequest {
  chatId: string;
  turnId: string;
  keyboard: boolean;
}

export const ForkChatRequestContext = createContext<
  ((request: ForkChatRequest) => void) | null
>(null);

export function turnSessionForkState(page: AcpChatPage, sessionId: string) {
  const session = page.sessions.find(({ id }) => id === sessionId);
  if (!session) return "absent";
  return session.residency === "hot" && session.capabilities.nativeSessionFork
    ? "forkable"
    : "present";
}
