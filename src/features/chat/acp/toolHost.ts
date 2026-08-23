import { applyAgentFileIntent, type AgentFileIntent } from "./agentFileOps";

export const CURATED_READ_TOOL_NAME = "sage.read_text" as const;
export const CURATED_APPLY_FILE_TOOL_NAME = "sage.apply_file" as const;

export interface ToolHost {
  canInvoke(toolId: string): boolean;
  applyFileIntent(intent: AgentFileIntent): Promise<void>;
}

export function createToolHost(): ToolHost {
  return {
    canInvoke(toolId) {
      return (
        toolId === CURATED_READ_TOOL_NAME ||
        toolId === CURATED_APPLY_FILE_TOOL_NAME
      );
    },
    applyFileIntent: applyAgentFileIntent,
  };
}
