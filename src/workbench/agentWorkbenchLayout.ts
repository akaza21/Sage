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
  navigationCollapsed = false,
  requestedWidths,
  prioritize = "left",
}: {
  viewportWidth: number;
  artifactExpanded: boolean;
  navigationCollapsed?: boolean;
  requestedWidths?: AgentDesktopSidebarWidths;
  prioritize?: AgentDesktopSidebarPriority;
}) {
  const desktopWidth = Math.max(INLINE_RESOURCE_MIN, viewportWidth);
  const navigationCap = Math.min(
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
    Math.round(262 + ((Math.max(480, viewportWidth) - 480) * 70) / 240),
  );
  const desktop = viewportWidth >= NAVIGATION_RESIZE_MIN;
  const navigationResizable = desktop && !navigationCollapsed;
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
  const navigationWidth = navigationCollapsed
    ? 54
    : clampWorkbenchSidebarWidth(navigationBounds, requested.leftRailWidth);
  const artifactInlineAvailable =
    desktop &&
    viewportWidth - navigationWidth >=
      AGENT_CHAT_MIN_WIDTH + 16 + AGENT_ARTIFACT_WIDTH.minWidth;
  const artifactInline = artifactExpanded && artifactInlineAvailable;
  const leftRailBounds = artifactInline
    ? agentDesktopLeftRailWidth(viewportWidth)
    : navigationBounds;
  const artifactBounds =
    navigationCollapsed && artifactInline
      ? {
          ...AGENT_ARTIFACT_WIDTH,
          maxWidth: Math.min(
            AGENT_ARTIFACT_WIDTH.maxWidth,
            viewportWidth - 54 - AGENT_CHAT_MIN_WIDTH - 8,
          ),
        }
      : agentDesktopArtifactWidth(
          artifactInline ? viewportWidth : desktopWidth,
        );
  const widths = navigationCollapsed
    ? {
        leftRailWidth: 54,
        artifactWidth: clampWorkbenchSidebarWidth(
          artifactBounds,
          requested.artifactWidth,
        ),
      }
    : artifactInline
      ? fitAgentDesktopSidebarWidths({
          viewportWidth,
          ...requested,
          prioritize,
        })
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
    compact: !desktop,
    artifactOverlay,
    artifactOverlayOnExpand: !artifactInlineAvailable,
    artifactModal,
    artifactOverlayWidth,
    artifactOverlayLeft: viewportWidth - artifactOverlayWidth,
    chatWidth,
    gridTemplateColumns: artifactInline
      ? `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr) 8px ${widths.artifactWidth}px`
      : `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr)`,
  };
}
