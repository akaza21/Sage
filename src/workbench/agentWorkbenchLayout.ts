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
const INLINE_RESOURCE_SPAN =
  AGENT_CHAT_MIN_WIDTH + 16 + AGENT_ARTIFACT_WIDTH.minWidth;
const INLINE_RESOURCE_MIN =
  AGENT_LEFT_RAIL_WIDTH.defaultWidth + INLINE_RESOURCE_SPAN;

export function agentWorkbenchLayout({
  viewportWidth,
  artifactExpanded,
  navigationCollapsed: navigationCollapsedRequest = false,
  requestedWidths,
  prioritize = "left",
}: {
  viewportWidth: number;
  artifactExpanded: boolean;
  navigationCollapsed?: boolean;
  requestedWidths?: Partial<AgentDesktopSidebarWidths>;
  prioritize?: AgentDesktopSidebarPriority;
}) {
  const desktopWidth = Math.max(INLINE_RESOURCE_MIN, viewportWidth);
  const navigationCap = Math.min(
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
    Math.round(262 + ((Math.max(480, viewportWidth) - 480) * 70) / 240),
  );
  const desktop = viewportWidth >= NAVIGATION_RESIZE_MIN;
  const defaults = initialAgentDesktopSidebarWidths(desktopWidth);
  const requested = {
    leftRailWidth:
      requestedWidths?.leftRailWidth ??
      Math.min(defaults.leftRailWidth, navigationCap),
    artifactWidth: requestedWidths?.artifactWidth ?? defaults.artifactWidth,
  };
  const artifactShare =
    requestedWidths?.artifactWidth ?? Math.round(viewportWidth * 0.45);
  const expandedNavigationBounds = {
    ...AGENT_LEFT_RAIL_WIDTH,
    defaultWidth: navigationCap,
    minWidth: desktop ? AGENT_LEFT_RAIL_WIDTH.minWidth : navigationCap,
    maxWidth: desktop
      ? Math.min(
          AGENT_LEFT_RAIL_WIDTH.maxWidth,
          viewportWidth - AGENT_CHAT_MIN_WIDTH - 8,
        )
      : navigationCap,
  };
  // Once the person drags either splitter, the allocator trades width between
  // the rail and the Files by resize priority, so a drag never folds the rail
  // it is sizing.
  const filesRoomBesideNavigation =
    viewportWidth -
    clampWorkbenchSidebarWidth(
      expandedNavigationBounds,
      requested.leftRailWidth,
    ) -
    AGENT_CHAT_MIN_WIDTH -
    16;
  const navigationFits =
    desktop &&
    filesRoomBesideNavigation >=
      (requestedWidths?.artifactWidth === undefined &&
      requestedWidths?.leftRailWidth === undefined
        ? Math.max(AGENT_ARTIFACT_WIDTH.minWidth, artifactShare)
        : AGENT_ARTIFACT_WIDTH.minWidth);
  const navigationCollapsed =
    navigationCollapsedRequest || (artifactExpanded && !navigationFits);
  const navigationResizable = desktop && !navigationCollapsed;
  const navigationBounds = navigationResizable
    ? expandedNavigationBounds
    : {
        ...expandedNavigationBounds,
        minWidth: navigationCap,
        maxWidth: navigationCap,
      };
  const artifactInlineAvailable =
    navigationCollapsedRequest || !navigationFits
      ? viewportWidth - AGENT_ICON_RAIL_WIDTH >= INLINE_RESOURCE_SPAN
      : true;
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
          artifactShare,
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
  const artifactOverlayWidth = artifactModal
    ? Math.min(420, viewportWidth - 16)
    : chatWidth;
  return {
    ...widths,
    navigationCollapsed,
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
