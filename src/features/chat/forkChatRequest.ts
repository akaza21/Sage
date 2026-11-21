import { createContext } from "react";

import type { AcpChatPage } from "../../domain/acp";

export interface ForkChatRequest {
  chatId: string;
  turnId: string;
  keyboard: boolean;
}

export interface RewindChatRequest {
  chatId: string;
  turnId: string;
  text: string;
  kind: "rewind";
}
export type ConversationRequest = ForkChatRequest | RewindChatRequest;
export const ConversationRequestContext = createContext<
  ((request: ConversationRequest) => void) | null
>(null);

export const ConversationActionContext = createContext<AcpChatPage | null>(
  null,
);
