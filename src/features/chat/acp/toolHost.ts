import {
  applyBrowseIntent,
  type HarnessBrowseIntent,
} from "../../preview/harnessBrowse";
export const CURATED_BROWSE_TOOL_NAME = "_sage/browse" as const;

export function invokeBrowseTool(
  toolId: string,
  intent: HarnessBrowseIntent,
): ReturnType<typeof applyBrowseIntent> {
  return toolId === CURATED_BROWSE_TOOL_NAME ? applyBrowseIntent(intent) : null;
}
