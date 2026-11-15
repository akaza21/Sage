import { useState, type ReactNode } from "react";
import AgentChatRuntime from "../workbench/AgentChatRuntime";
import { AgentChatRuntimeContext } from "../workbench/AgentChatRuntimeContext";

export function AgentChatRuntimeHarness({ children }: { children: ReactNode }) {
  const [ids, setIds] = useState<readonly string[]>([]);
  return (
    <AgentChatRuntimeContext value={setIds}>
      <AgentChatRuntime ids={ids} visible />
      {children}
    </AgentChatRuntimeContext>
  );
}
