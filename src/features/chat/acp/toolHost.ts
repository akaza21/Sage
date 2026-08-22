export const CURATED_READ_TOOL_NAME = "sage.read_text" as const;

export interface ToolHost {
  canInvoke(toolId: string): boolean;
}

export function createToolHost(): ToolHost {
  return {
    canInvoke(toolId) {
      return toolId === CURATED_READ_TOOL_NAME;
    },
  };
}
