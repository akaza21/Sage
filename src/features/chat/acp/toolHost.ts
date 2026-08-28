import {
  applyBrowseIntent,
  type HarnessBrowseIntent,
} from "../../preview/harnessBrowse";
import { applyAgentFileIntent, type AgentFileIntent } from "./agentFileOps";

export const CURATED_READ_TOOL_NAME = "sage.read_text" as const;
export const CURATED_APPLY_FILE_TOOL_NAME = "sage.apply_file" as const;
export const CURATED_BROWSE_TOOL_NAME = "sage.browse" as const;

export interface ToolHost {
  canInvoke(toolId: string): boolean;
  applyFileIntent(intent: AgentFileIntent): Promise<void>;
  applyBrowseIntent(
    intent: HarnessBrowseIntent,
  ): ReturnType<typeof applyBrowseIntent>;
  invoke(
    toolId: string,
    intent: HarnessBrowseIntent,
  ): ReturnType<typeof applyBrowseIntent>;
}

export function createToolHost(): ToolHost {
  return {
    canInvoke(toolId) {
      return (
        toolId === CURATED_READ_TOOL_NAME ||
        toolId === CURATED_APPLY_FILE_TOOL_NAME ||
        toolId === CURATED_BROWSE_TOOL_NAME
      );
    },
    applyFileIntent: applyAgentFileIntent,
    applyBrowseIntent,
    invoke(toolId, intent) {
      if (toolId !== CURATED_BROWSE_TOOL_NAME) return null;
      return applyBrowseIntent(intent);
    },
  };
}
