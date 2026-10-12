import {
  AGENT_ARTIFACT_WIDTH,
  AGENT_CHAT_MIN_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  agentDesktopArtifactWidth,
  agentDesktopLeftRailWidth,
  clampWorkbenchSidebarWidth,
  fitAgentDesktopSidebarWidths,
  initialAgentDesktopSidebarWidths,
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";

const NAVIGATION_RESIZE_MIN = 761;
const INLINE_RESOURCE_MIN =
  AGENT_LEFT_RAIL_WIDTH.defaultWidth +
  AGENT_CHAT_MIN_WIDTH +
  16 +
  AGENT_ARTIFACT_WIDTH.minWidth;

export function agentWorkbenchLayout({
  viewportWidth,
  artifactExpanded,
  requestedWidths,
  prioritize = "left",
}: {
  viewportWidth: number;
  artifactExpanded: boolean;
  requestedWidths?: AgentDesktopSidebarWidths;
  prioritize?: AgentDesktopSidebarPriority;
}) {
  const desktopWidth = Math.max(INLINE_RESOURCE_MIN, viewportWidth);
  const navigationCap = Math.min(
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
    Math.round(262 + ((Math.max(480, viewportWidth) - 480) * 70) / 240),
  );
  const navigationResizable = viewportWidth >= NAVIGATION_RESIZE_MIN;
  const defaults = initialAgentDesktopSidebarWidths(desktopWidth);
  const requested = requestedWidths ?? {
    ...defaults,
    leftRailWidth: Math.min(defaults.leftRailWidth, navigationCap),
  };
  const navigationBounds = {
    ...AGENT_LEFT_RAIL_WIDTH,
    defaultWidth: navigationCap,
    minWidth: navigationResizable
      ? AGENT_LEFT_RAIL_WIDTH.minWidth
      : navigationCap,
    maxWidth: navigationResizable
      ? Math.min(
          AGENT_LEFT_RAIL_WIDTH.maxWidth,
          viewportWidth - AGENT_CHAT_MIN_WIDTH - 8,
        )
      : navigationCap,
  };
  const navigationWidth = clampWorkbenchSidebarWidth(
    navigationBounds,
    requested.leftRailWidth,
  );
  const artifactInlineAvailable =
    navigationResizable &&
    viewportWidth - navigationWidth >=
      AGENT_CHAT_MIN_WIDTH + 16 + AGENT_ARTIFACT_WIDTH.minWidth;
  const artifactInline = artifactExpanded && artifactInlineAvailable;
  const leftRailBounds = artifactInline
    ? agentDesktopLeftRailWidth(viewportWidth)
    : navigationBounds;
  const artifactBounds = agentDesktopArtifactWidth(
    artifactInline ? viewportWidth : desktopWidth,
  );
  const widths = artifactInline
    ? fitAgentDesktopSidebarWidths({ viewportWidth, ...requested, prioritize })
    : {
        leftRailWidth: clampWorkbenchSidebarWidth(
          leftRailBounds,
          requested.leftRailWidth,
        ),
        artifactWidth: clampWorkbenchSidebarWidth(
          artifactBounds,
          requested.artifactWidth,
        ),
      };
  const leadingSplitter = navigationResizable ? 8 : 0;
  const chatWidth =
    viewportWidth -
    widths.leftRailWidth -
    leadingSplitter -
    (artifactInline ? widths.artifactWidth + 8 : 0);
  const artifactOverlay = artifactExpanded && !artifactInline;
  const artifactModal = artifactOverlay && chatWidth < 320;
  const artifactOverlayWidth = Math.min(
    420,
    artifactModal ? viewportWidth - 16 : chatWidth,
  );
  return {
    ...widths,
    leftRailBounds,
    artifactBounds,
    navigationResizable,
    artifactOverlay,
    artifactOverlayOnExpand: !artifactInlineAvailable,
    artifactModal,
    artifactOverlayWidth,
    artifactOverlayLeft: viewportWidth - artifactOverlayWidth,
    chatWidth,
    gridTemplateColumns: artifactInline
      ? `${widths.leftRailWidth}px 8px minmax(0, 1fr) 8px ${widths.artifactWidth}px`
      : `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr)`,
  };
}
