import { useState, type ReactNode } from "react";
import AgentChatRuntime from "../workbench/AgentChatRuntime";
import { AgentChatRuntimeContext } from "../workbench/AgentChatRuntimeContext";

/** The real headless owner around an isolated Agent presentation in tests. */
export function AgentChatRuntimeHarness({ children }: { children: ReactNode }) {
  const [ids, setIds] = useState<readonly string[]>([]);
  return (
    <AgentChatRuntimeContext value={setIds}>
      <AgentChatRuntime ids={ids} visible />
      {children}
    </AgentChatRuntimeContext>
  );
}
