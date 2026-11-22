import { createContext } from "react";

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
