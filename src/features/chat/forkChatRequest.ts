import { createContext } from "react";

export interface ForkChatRequest {
  chatId: string;
  turnId: string;
  keyboard: boolean;
}

export const ForkChatRequestContext = createContext<
  ((request: ForkChatRequest) => void) | null
>(null);
