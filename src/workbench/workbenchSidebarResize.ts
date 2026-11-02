export type WorkbenchSidebarWidth = {
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
};

export const AGENT_LEFT_RAIL_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 385,
  minWidth: 180,
  maxWidth: 460,
};

export const EDITOR_TOOL_RAIL_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 272,
  minWidth: 200,
  maxWidth: 420,
};

export const EDITOR_ACTIVITY_BAR_WIDTH = 48;

export const AGENT_CHAT_MIN_WIDTH = 360;

export const AGENT_SPLITTER_WIDTH = 8;
export const AGENT_COLLAPSED_RAIL_WIDTH = 54;
const AGENT_ARTIFACT_FLOOR_WIDTH = 160;

export const AGENT_ARTIFACT_WIDTH: WorkbenchSidebarWidth = {
  defaultWidth: 1046,
  minWidth: 280,
  maxWidth: 1200,
};

export type AgentDesktopSidebarWidths = {
  leftRailWidth: number;
  artifactWidth: number;
};

export type AgentDesktopSidebarPriority = "left" | "artifact";

function desktopSidebarBounds(viewportWidth: number): {
  leftRail: WorkbenchSidebarWidth;
  artifact: WorkbenchSidebarWidth;
  available: number;
} {
  const available = Math.max(
    0,
    Math.round(viewportWidth) - AGENT_CHAT_MIN_WIDTH - AGENT_SPLITTER_WIDTH * 2,
  );
  const artifactMin = Math.min(
    AGENT_ARTIFACT_WIDTH.minWidth,
    Math.max(
      AGENT_ARTIFACT_FLOOR_WIDTH,
      available - AGENT_LEFT_RAIL_WIDTH.minWidth,
    ),
  );
  const artifactMax = Math.min(
    AGENT_ARTIFACT_WIDTH.maxWidth,
    Math.max(artifactMin, available - AGENT_LEFT_RAIL_WIDTH.minWidth),
  );
  const artifact: WorkbenchSidebarWidth = {
    ...AGENT_ARTIFACT_WIDTH,
    defaultWidth: Math.min(AGENT_ARTIFACT_WIDTH.defaultWidth, artifactMax),
    minWidth: artifactMin,
    maxWidth: artifactMax,
  };
  const leftRail: WorkbenchSidebarWidth = {
    ...AGENT_LEFT_RAIL_WIDTH,
    defaultWidth: Math.min(
      AGENT_LEFT_RAIL_WIDTH.defaultWidth,
      available - artifact.minWidth,
    ),
    maxWidth: Math.min(
      AGENT_LEFT_RAIL_WIDTH.maxWidth,
      available - artifact.minWidth,
    ),
  };
  return { leftRail, artifact, available };
}

export function agentDesktopLeftRailWidth(viewportWidth: number) {
  return desktopSidebarBounds(viewportWidth).leftRail;
}

export function agentDesktopArtifactWidth(viewportWidth: number) {
  return desktopSidebarBounds(viewportWidth).artifact;
}

export function fitAgentDesktopSidebarWidths({
  viewportWidth,
  leftRailWidth,
  artifactWidth,
  prioritize,
}: AgentDesktopSidebarWidths & {
  viewportWidth: number;
  prioritize: AgentDesktopSidebarPriority;
}): AgentDesktopSidebarWidths {
  const { leftRail, artifact, available } = desktopSidebarBounds(viewportWidth);
  let left = clamp(leftRail, leftRailWidth);
  let right = clamp(artifact, artifactWidth);
  if (left + right <= available) {
    return { leftRailWidth: left, artifactWidth: right };
  }
  if (prioritize === "left") {
    right = clamp(artifact, available - left);
  } else {
    left = clamp(leftRail, available - right);
  }
  return { leftRailWidth: left, artifactWidth: right };
}

export function initialAgentDesktopSidebarWidths(
  viewportWidth: number,
): AgentDesktopSidebarWidths {
  return fitAgentDesktopSidebarWidths({
    viewportWidth,
    leftRailWidth: initialAgentLeftRailWidth(viewportWidth),
    artifactWidth: initialAgentArtifactWidth(viewportWidth),
    prioritize: "left",
  });
}

export function initialAgentLeftRailWidth(viewportWidth: number): number {
  return clampWorkbenchSidebarWidth(
    agentDesktopLeftRailWidth(viewportWidth),
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
  );
}

export function initialAgentArtifactWidth(viewportWidth: number): number {
  return clampWorkbenchSidebarWidth(
    agentDesktopArtifactWidth(viewportWidth),
    Math.round(viewportWidth * 0.45),
  );
}

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
