import { createContext } from "react";

export const AgentChatRuntimeContext = createContext<
  ((ids: readonly string[]) => void) | null
>(null);
