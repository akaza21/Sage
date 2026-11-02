import {
  AGENT_ARTIFACT_WIDTH,
  AGENT_CHAT_MIN_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  AGENT_COLLAPSED_RAIL_WIDTH,
  AGENT_SPLITTER_WIDTH,
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
  // Shrink the split to its declared floors before resorting to an overlay.
  // If even the rail's floor will not fit, temporarily show its icon strip.
  const inlineMinimum =
    AGENT_CHAT_MIN_WIDTH +
    AGENT_ARTIFACT_WIDTH.minWidth +
    AGENT_SPLITTER_WIDTH * 2;
  const resourceNavigationCollapsed =
    navigationCollapsed ||
    (viewportWidth >= AGENT_COLLAPSED_RAIL_WIDTH + inlineMinimum &&
      viewportWidth < AGENT_LEFT_RAIL_WIDTH.minWidth + inlineMinimum);
  const navigationFits =
    viewportWidth >= AGENT_LEFT_RAIL_WIDTH.minWidth + inlineMinimum;
  const collapsed =
    navigationCollapsed || (artifactExpanded && resourceNavigationCollapsed);
  const desktopWidth = Math.max(INLINE_RESOURCE_MIN, viewportWidth);
  const navigationCap = Math.min(
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
    Math.round(262 + ((Math.max(480, viewportWidth) - 480) * 70) / 240),
  );
  const desktop = viewportWidth >= NAVIGATION_RESIZE_MIN;
  const navigationResizable = desktop && !collapsed;
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
  const artifactInlineAvailable =
    viewportWidth >=
    (resourceNavigationCollapsed
      ? AGENT_COLLAPSED_RAIL_WIDTH
      : AGENT_LEFT_RAIL_WIDTH.minWidth) +
      inlineMinimum;
  const artifactInline = artifactExpanded && artifactInlineAvailable;
  const leftRailBounds = artifactInline
    ? agentDesktopLeftRailWidth(viewportWidth)
    : navigationBounds;
  const artifactBounds =
    collapsed && artifactInline
      ? {
          ...AGENT_ARTIFACT_WIDTH,
          maxWidth: Math.min(
            AGENT_ARTIFACT_WIDTH.maxWidth,
            viewportWidth -
              AGENT_COLLAPSED_RAIL_WIDTH -
              AGENT_CHAT_MIN_WIDTH -
              AGENT_SPLITTER_WIDTH,
          ),
        }
      : agentDesktopArtifactWidth(
          artifactInline ? viewportWidth : desktopWidth,
        );
  const widths = collapsed
    ? {
        leftRailWidth: AGENT_COLLAPSED_RAIL_WIDTH,
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
    navigationCollapsed: collapsed,
    navigationFits,
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
