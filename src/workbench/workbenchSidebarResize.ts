export type WorkbenchSidebarWidth = {
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
};

export const AGENT_LEFT_RAIL_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 248,
  minWidth: 180,
  maxWidth: 380,
};

export const EDITOR_TOOL_RAIL_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 272,
  minWidth: 200,
  maxWidth: 420,
};

export const EDITOR_ACTIVITY_BAR_WIDTH = 48;

export const AGENT_ARTIFACT_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 360,
  minWidth: 280,
  maxWidth: 460,
};

type ResizeInput =
  | { pointerOffset: number }
  | { currentWidth?: number; key: string; shiftKey?: boolean };

export function resizeWorkbenchSidebar(
  width: WorkbenchSidebarWidth,
  input: ResizeInput,
): number {
  if ("pointerOffset" in input) {
    return clamp(width, input.pointerOffset);
  }
  if (input.key === "Home") return width.minWidth;
  if (input.key === "End") return width.maxWidth;
  const currentWidth = clamp(width, input.currentWidth ?? width.defaultWidth);
  const step = input.shiftKey ? 20 : 8;
  if (input.key === "ArrowLeft") return clamp(width, currentWidth - step);
  if (input.key === "ArrowRight") return clamp(width, currentWidth + step);
  return currentWidth;
}

export function clampWorkbenchSidebarWidth(
  width: WorkbenchSidebarWidth,
  value: number,
): number {
  return clamp(width, value);
}

function clamp(width: WorkbenchSidebarWidth, value: number): number {
  if (!Number.isFinite(value)) return width.defaultWidth;
  return Math.min(width.maxWidth, Math.max(width.minWidth, Math.round(value)));
}
