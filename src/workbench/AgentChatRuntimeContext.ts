import { createContext } from "react";

/** At most two presented Chat slots report to a runtime outside sleeping UI. */
export const AgentChatRuntimeContext = createContext<
  ((ids: readonly string[]) => void) | null
>(null);
