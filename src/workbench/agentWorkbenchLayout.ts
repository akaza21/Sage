import {
  AGENT_ICON_RAIL_WIDTH,
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
export const AGENT_INLINE_RESOURCE_MIN_WIDTH =
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
  const desktopWidth = Math.max(AGENT_INLINE_RESOURCE_MIN_WIDTH, viewportWidth);
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
    ? AGENT_ICON_RAIL_WIDTH
    : clampWorkbenchSidebarWidth(navigationBounds, requested.leftRailWidth);
  const artifactInlineAvailable =
    viewportWidth >= AGENT_INLINE_RESOURCE_MIN_WIDTH &&
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
            viewportWidth - AGENT_ICON_RAIL_WIDTH - AGENT_CHAT_MIN_WIDTH - 8,
          ),
        }
      : agentDesktopArtifactWidth(
          artifactInline ? viewportWidth : desktopWidth,
        );
  const widths = navigationCollapsed
    ? {
        leftRailWidth: AGENT_ICON_RAIL_WIDTH,
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
  const artifactOverlayWidth = Math.min(420, viewportWidth - 16);
  return {
    ...widths,
    navigationCollapsed,
    navigationFits:
      viewportWidth >=
      AGENT_LEFT_RAIL_WIDTH.minWidth +
        AGENT_CHAT_MIN_WIDTH +
        16 +
        AGENT_ARTIFACT_WIDTH.minWidth,
    leftRailBounds,
    artifactBounds,
    navigationResizable,
    compact: !desktop,
    artifactOverlay,
    artifactOverlayOnExpand: !artifactInlineAvailable,
    artifactModal: artifactOverlay,
    artifactOverlayWidth,
    artifactOverlayLeft: viewportWidth - artifactOverlayWidth,
    chatWidth,
    contentStart: widths.leftRailWidth + leadingSplitter,
    gridTemplateColumns: artifactInline
      ? `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr) 8px ${widths.artifactWidth}px`
      : `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr)`,
  };
}
